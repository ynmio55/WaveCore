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
                gl.bufferData(
                    gl.ARRAY_BUFFER,
                    new Float32Array([-1, -1, 1, -1, 0, 1]),
                    gl.STATIC_DRAW
                );
                gl.enableVertexAttribArray(0);
                gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
                gl.clearColor(0.1, 0.2, 0.3, 1.0);
                gl.clear(gl.COLOR_BUFFER_BIT);
                gl.drawArrays(gl.TRIANGLES, 0, 3);
                let ibo = gl.createBuffer();
                gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, ibo);
                gl.bufferData(
                    gl.ELEMENT_ARRAY_BUFFER,
                    new Uint16Array([0, 1, 2]),
                    gl.STATIC_DRAW
                );
                gl.drawElements(gl.TRIANGLES, 3, gl.UNSIGNED_SHORT, 0);
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
        assert!(stream.iter().any(|c| matches!(
            c,
            wavecore_render::WebGlCommand::UploadElementArrayBuffer { data, .. }
                if data == &vec![0, 1, 2]
        )));
        assert!(stream.iter().any(|c| matches!(
            c,
            wavecore_render::WebGlCommand::DrawElements { mode, count, .. }
                if *mode == 4 && *count == 3
        )));
    }

    #[test]
    fn typed_arrays_coerce_values_and_support_indexing() {
        let mut vm = VM::new();
        let result = eval_script(
            r#"
                let a = new Uint8Array([257, -1, 3]);
                let b = new Float32Array([1.25, 2.5]);
                a[0] + a[1] + a.length + b[0];
            "#,
            &mut vm,
        )
        .unwrap();
        assert_eq!(result, JsValue::Number(260.25));
    }

    #[test]
    fn request_animation_frame_runs_with_timestamp() {
        let root = Rc::new(RefCell::new(Node::document(vec![])));
        let bridge = DomBridge::new(root);
        let mut vm = VM::new();
        bridge.attach_to_vm(&mut vm);

        eval_script(
            r#"
                let frameTime = -1;
                requestAnimationFrame(function(ts) {
                    frameTime = ts;
                });
            "#,
            &mut vm,
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(bridge.dispatch_due_timers(&mut vm), 1);
        let result = eval_script("frameTime;", &mut vm).unwrap();
        assert!(matches!(result, JsValue::Number(v) if v >= 0.0));
    }

    #[test]
    fn performance_now_and_microtasks_are_available() {
        let root = Rc::new(RefCell::new(Node::document(vec![])));
        let bridge = DomBridge::new(root);
        let mut vm = VM::new();
        bridge.attach_to_vm(&mut vm);

        eval_script(
            r#"
                let observed = 0;
                let perfOk = performance.now() >= 0 ? 1 : 0;
                queueMicrotask(function() { observed = 7; });
            "#,
            &mut vm,
        )
        .unwrap();

        // eval_script drains the VM microtask queue after the current script turn.
        let result = eval_script("observed + perfOk;", &mut vm).unwrap();
        assert_eq!(result, JsValue::Number(8.0));
    }

    #[test]
    fn set_interval_repeats_and_can_be_cleared() {
        let root = Rc::new(RefCell::new(Node::document(vec![])));
        let bridge = DomBridge::new(root);
        let mut vm = VM::new();
        bridge.attach_to_vm(&mut vm);

        eval_script(
            r#"
                let ticks = 0;
                let intervalId = setInterval(function() {
                    ticks = ticks + 1;
                }, 1);
            "#,
            &mut vm,
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(3));
        assert_eq!(bridge.dispatch_due_timers(&mut vm), 1);
        std::thread::sleep(std::time::Duration::from_millis(3));
        assert_eq!(bridge.dispatch_due_timers(&mut vm), 1);
        assert_eq!(eval_script("ticks;", &mut vm).unwrap(), JsValue::Number(2.0));

        eval_script("clearInterval(intervalId);", &mut vm).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3));
        assert_eq!(bridge.dispatch_due_timers(&mut vm), 0);
    }

}
