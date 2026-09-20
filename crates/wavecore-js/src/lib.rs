pub mod ast;
pub mod bytecode;
pub mod dom_bridge;
pub mod lexer;
pub mod parser;
pub mod value;
pub mod vm;

pub use dom_bridge::DomBridge;
pub use value::{JsObject, JsValue};
pub use vm::VM;

pub fn eval_script(script: &str, vm: &mut VM) -> Result<JsValue, String> {
    let mut lexer = lexer::Lexer::new(script);
    let tokens = lexer.tokenize()?;
    let mut parser = parser::Parser::new(tokens);
    let statements = parser.parse()?;
    let compiler = bytecode::Compiler::new();
    let chunks = compiler.compile(&statements)?;
    vm.execute(chunks)
}

pub fn execute_script(script: &str) -> Result<JsValue, String> {
    let mut vm = VM::new();
    eval_script(script, &mut vm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use wavecore_dom::Node;

    #[test]
    fn arithmetic_and_variables() {
        let mut vm = VM::new();
        let code = "let a = 10; let b = 20; let c = a + b * 2; c;";
        let res = eval_script(code, &mut vm).unwrap();
        assert_eq!(res, JsValue::Number(50.0));
    }

    #[test]
    fn functions_and_conditionals() {
        let mut vm = VM::new();
        let code = r#"
            function factorial(n) {
                if (n <= 1) {
                    return 1;
                }
                return n * factorial(n - 1);
            }
            factorial(5);
        "#;
        let res = eval_script(code, &mut vm).unwrap();
        assert_eq!(res, JsValue::Number(120.0));
    }

    #[test]
    fn loops_and_accumulation() {
        let mut vm = VM::new();
        let code = r#"
            let sum = 0;
            for (let i = 1; i <= 10; i = i + 1) {
                sum = sum + i;
            }
            sum;
        "#;
        let res = eval_script(code, &mut vm).unwrap();
        assert_eq!(res, JsValue::Number(55.0));
    }

    #[test]
    fn dom_bridge_manipulation() {
        let root = Rc::new(RefCell::new(Node::element(
            "div",
            vec![
                Node::element_with_attributes(
                    "h1",
                    [("id".to_string(), "title".to_string())].into(),
                    vec![Node::text("Original")],
                ),
            ],
        )));

        let bridge = DomBridge::new(root.clone());
        let mut vm = VM::new();
        bridge.attach_to_vm(&mut vm);

        let script = r#"
            let h1 = document.getElementById("title");
            h1.setInnerText("Updated by Pulse JS");
        "#;
        eval_script(script, &mut vm).unwrap();

        assert_eq!(root.borrow().inner_text(), "Updated by Pulse JS");
    }
    #[test]
    fn dom_bridge_timer_runs_on_event_loop() {
        let root = Rc::new(RefCell::new(Node::document(vec![])));
        let bridge = DomBridge::new(root);
        let mut vm = VM::new();
        bridge.attach_to_vm(&mut vm);

        eval_script(
            r#"setTimeout(function() { console.log("timer-fired"); }, 0);"#,
            &mut vm,
        )
        .unwrap();

        assert_eq!(bridge.dispatch_due_timers(&mut vm), 1);
        assert!(vm.console_output.iter().any(|line| line == "timer-fired"));
    }

    #[test]
    fn fetch_response_exposes_real_status() {
        let root = Rc::new(RefCell::new(Node::document(vec![])));
        let bridge = DomBridge::with_url(root, "https://wavecore.local/");
        let mut vm = VM::new();
        bridge.attach_to_vm(&mut vm);

        let result = eval_script(
            r#"
                let observed = 0;
                fetch("data:text/plain,hello").then(function(response) {
                    observed = response.status;
                    return observed;
                });
                observed;
            "#,
            &mut vm,
        )
        .unwrap();

        assert_eq!(result, JsValue::Number(200.0));
    }

    #[test]
    fn conditional_and_short_circuit_operators() {
        let mut vm = VM::new();
        let res = eval_script(
            r#"
                let side = 0;
                false && (side = 1);
                true || (side = 2);
                let a = true ? 10 : 20;
                let b = false ? 30 : 40;
                a + b + side;
            "#,
            &mut vm,
        )
        .unwrap();
        assert_eq!(res, JsValue::Number(50.0));
    }

    #[test]
    fn webgl_records_real_buffer_and_draw_commands() {
        let mut attrs = std::collections::BTreeMap::new();
        attrs.insert("id".to_string(), "glcanvas".to_string());
        let canvas = Node::element_with_attributes("canvas", attrs, vec![]);
        let root = Rc::new(RefCell::new(Node::document(vec![canvas])));
        let bridge = DomBridge::new(root);
        let mut vm = VM::new();
        bridge.attach_to_vm(&mut vm);

        eval_script(
            r#"
                let canvas = document.getElementById("glcanvas");
                let gl = canvas.getContext("webgl");
                let buffer = gl.createBuffer();
                gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
                gl.bufferData(gl.ARRAY_BUFFER, [-1, -1, 1, -1, 0, 1], gl.STATIC_DRAW);
                gl.enableVertexAttribArray(0);
                gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
                gl.clearColor(0.1, 0.2, 0.3, 1.0);
                gl.clear(gl.COLOR_BUFFER_BIT);
                gl.drawArrays(gl.TRIANGLES, 0, 3);
            "#,
            &mut vm,
        )
        .unwrap();

        let commands = bridge.webgl_commands_snapshot();
        let stream = commands.values().next().expect("webgl command stream");
        assert!(stream.iter().any(|c| matches!(
            c,
            wavecore_render::WebGlCommand::UploadArrayBuffer { data, .. } if data.len() == 6
        )));
        assert!(stream.iter().any(|c| matches!(
            c,
            wavecore_render::WebGlCommand::DrawArrays { mode, count, .. }
                if *mode == 4 && *count == 3
        )));
    }

}
