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
}
