use std::cell::RefCell;
use std::rc::Rc;
use wavecore_dom::Node;
use wavecore_js::dom_bridge::DomBridge;
use wavecore_js::execute_script;
use wavecore_js::value::JsValue;
use wavecore_js::vm::VM;

#[test]
fn test_true_lexical_closures() {
    let script = r#"
        function makeCounter(start) {
            let count = start;
            return function() {
                count = count + 1;
                return count;
            };
        }

        let c1 = makeCounter(10);
        let c2 = makeCounter(100);

        let v1_a = c1();
        let v1_b = c1();
        let v2_a = c2();
        let v1_c = c1();

        v1_a + v1_b + v2_a + v1_c;
    "#;

    let res = execute_script(script).expect("Execution failed");
    // v1_a = 11, v1_b = 12, v2_a = 101, v1_c = 13 => 11 + 12 + 101 + 13 = 137
    assert_eq!(res.to_number(), 137.0);
}

#[test]
fn test_try_catch_finally_and_throw() {
    let script = r#"
        let status = 0;
        try {
            status = 1;
            throw 404;
            status = 2; // Should be skipped
        } catch (err) {
            status = status + err; // 1 + 404 = 405
        } finally {
            status = status + 10; // 405 + 10 = 415
        }
        status;
    "#;

    let res = execute_script(script).expect("Execution failed");
    assert_eq!(res.to_number(), 415.0);
}

#[test]
fn test_arrays_and_indexing() {
    let script = r#"
        let nums = [10, 20, 30];
        nums.push(40);
        nums.push(50);
        
        let first = nums[0];
        let last = nums.pop();
        let len = nums.length;
        
        nums[1] = 99;
        let modified = nums[1];
        
        first + last + len + modified;
    "#;

    let res = execute_script(script).expect("Execution failed");
    // first=10, last=50, len=4, modified=99 => 10 + 50 + 4 + 99 = 163
    assert_eq!(res.to_number(), 163.0);
}

#[test]
fn test_array_higher_order_methods() {
    let script = r#"
        let items = [1, 2, 3, 4];
        let doubled = items.map(function(x) {
            return x * 2;
        });

        doubled.join("-");
    "#;

    let res = execute_script(script).expect("Execution failed");
    assert_eq!(res.to_js_string(), "2-4-6-8");
}

#[test]
fn test_objects_and_properties() {
    let script = r#"
        let user = {
            name: "WaveCore",
            version: 1,
            fast: true
        };

        user.version = user.version + 1;
        user["engine"] = "Pulse";

        user.name + " " + user.version + " " + user.engine;
    "#;

    let res = execute_script(script).expect("Execution failed");
    assert_eq!(res.to_js_string(), "WaveCore 2 Pulse");
}

#[test]
fn test_strings_and_typeof() {
    let script = r#"
        let str = "  WaveCore Browser  ";
        let trimmed = str.trim();
        let lower = trimmed.toLowerCase();
        let hasCore = lower.includes("core");
        let t = typeof 123;
        let t_str = typeof "hello";
        let t_fn = typeof function() {};

        trimmed + " | " + lower + " | " + hasCore + " | " + t + " | " + t_str + " | " + t_fn;
    "#;

    let res = execute_script(script).expect("Execution failed");
    assert_eq!(
        res.to_js_string(),
        "WaveCore Browser | wavecore browser | true | number | string | function"
    );
}

#[test]
fn test_promises_and_chaining() {
    let script = r#"
        let p = Promise.resolve(21);
        let res = 0;
        p.then(function(val) {
            return val * 2;
        }).then(function(val2) {
            res = val2;
            return res;
        });
        res;
    "#;

    let res = execute_script(script).expect("Execution failed");
    assert_eq!(res.to_number(), 42.0);
}

#[test]
fn test_web_apis_window_location_and_history() {
    let doc = Node::document(vec![
        Node::element_with_attributes("div", [("id".into(), "app".into())].into(), vec![]),
    ]);
    let root = Rc::new(RefCell::new(doc));
    let bridge = DomBridge::with_url(root, "https://wavecore.dev/docs/intro?tab=guide#install");
    let mut vm = VM::new();
    bridge.attach_to_vm(&mut vm);

    let script = r#"
        let url_full = window.location.href;
        let proto = location.protocol;
        let host = location.host;
        let path = location.pathname;
        let hist_len = history.length;

        proto + "//" + host + path + " (hist: " + hist_len + ")";
    "#;

    let tokens = wavecore_js::lexer::Lexer::new(script).tokenize().unwrap();
    let stmts = wavecore_js::parser::Parser::new(tokens).parse().unwrap();
    let chunks = wavecore_js::bytecode::Compiler::new().compile(&stmts).unwrap();
    let res = vm.execute(chunks).unwrap();

    assert_eq!(res.to_js_string(), "https://wavecore.dev/docs/intro (hist: 1)");
}

#[test]
fn test_web_apis_dom_manipulation_and_classlist() {
    let doc = Node::document(vec![
        Node::element_with_attributes("div", [("id".into(), "container".into())].into(), vec![]),
    ]);
    let root = Rc::new(RefCell::new(doc));
    let bridge = DomBridge::new(root.clone());
    let mut vm = VM::new();
    bridge.attach_to_vm(&mut vm);

    let script = r#"
        let container = document.getElementById("container");
        container.classList.add("header-box");
        container.classList.add("active");
        let hasActive = container.classList.contains("active");

        let newCard = document.createElement("p");
        newCard.setInnerText("Dynamic paragraph from JS");
        container.appendChild(newCard);

        hasActive;
    "#;

    let tokens = wavecore_js::lexer::Lexer::new(script).tokenize().unwrap();
    let stmts = wavecore_js::parser::Parser::new(tokens).parse().unwrap();
    let chunks = wavecore_js::bytecode::Compiler::new().compile(&stmts).unwrap();
    let res = vm.execute(chunks).unwrap();

    assert_eq!(res, JsValue::Boolean(true));

    // Verify DOM tree directly in Rust!
    let borrowed = root.borrow();
    let container_node = borrowed.find_by_id("container").expect("Container not found");
    assert!(container_node.class_name().unwrap().contains("header-box"));
    assert!(container_node.class_name().unwrap().contains("active"));
    assert!(container_node.inner_text().contains("Dynamic paragraph from JS"));
}

#[test]
fn test_web_apis_timers_and_fetch() {
    let doc = Node::document(vec![]);
    let root = Rc::new(RefCell::new(doc));
    let bridge = DomBridge::new(root);
    let mut vm = VM::new();
    bridge.attach_to_vm(&mut vm);

    let script = r#"
        let timerId = setTimeout(function() {}, 500);
        let u = new URL("https://example.com/api/data?query=fast#view");
        
        let fetchPromise = fetch("data:text/plain;charset=utf-8,WaveCoreRocks");
        let fetchResult = "";
        fetchPromise.then(function(resp) {
            return resp.text();
        }).then(function(body) {
            fetchResult = body;
            return body;
        });

        timerId > 0 && u.host == "example.com" && fetchResult == "WaveCoreRocks";
    "#;

    let tokens = wavecore_js::lexer::Lexer::new(script).tokenize().unwrap();
    let stmts = wavecore_js::parser::Parser::new(tokens).parse().unwrap();
    let chunks = wavecore_js::bytecode::Compiler::new().compile(&stmts).unwrap();
    let res = vm.execute(chunks).unwrap();

    assert_eq!(res, JsValue::Boolean(true));
}
