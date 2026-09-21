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

        assert_eq!(result, JsValue::Number(0.0));
        assert_eq!(eval_script("observed;", &mut vm).unwrap(), JsValue::Number(200.0));
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

    #[test]
    fn object_helpers_and_array_from_work() {
        let mut vm = VM::new();
        let result = eval_script(
            r#"
                let src = { b: 2, a: 1 };
                let dst = Object.assign({ c: 3 }, src);
                let keys = Object.keys(dst);
                let chars = Array.from("WC");
                keys.length + chars.length + dst.a + dst.b + dst.c;
            "#,
            &mut vm,
        )
        .unwrap();
        assert_eq!(result, JsValue::Number(11.0));
    }

    #[test]
    fn json_parse_and_stringify_round_trip_objects_and_arrays() {
        let mut vm = VM::new();
        let result = eval_script(
            r#"
                let obj = JSON.parse("{\"name\":\"WaveCore\",\"values\":[1,2,3],\"ok\":true}");
                let encoded = JSON.stringify(obj);
                let again = JSON.parse(encoded);
                again.name + ":" + again.values[1] + ":" + again.ok;
            "#,
            &mut vm,
        )
        .unwrap();
        assert_eq!(result, JsValue::String("WaveCore:2:true".to_string()));
    }

    #[test]
    fn promise_all_and_race_resolve_immediate_values() {
        let mut vm = VM::new();
        let result = eval_script(
            r#"
                let total = 0;
                Promise.all([Promise.resolve(2), 3, Promise.resolve(5)])
                    .then(function(values) {
                        total = values[0] + values[1] + values[2];
                    });
                let winner = 0;
                Promise.race([Promise.resolve(9), Promise.resolve(10)])
                    .then(function(value) {
                        winner = value;
                    });
                total + winner;
            "#,
            &mut vm,
        )
        .unwrap();
        assert_eq!(result, JsValue::Number(0.0));
        assert_eq!(eval_script("total + winner;", &mut vm).unwrap(), JsValue::Number(19.0));
    }

    #[test]
    fn fetch_response_json_returns_parsed_object() {
        let root = Rc::new(RefCell::new(Node::document(vec![])));
        let bridge = DomBridge::with_url(root, "https://wavecore.local/");
        let mut vm = VM::new();
        bridge.attach_to_vm(&mut vm);

        let result = eval_script(
            r#"
                let value = 0;
                fetch("data:application/json,{\"value\":42}")
                    .then(function(response) {
                        return response.json();
                    })
                    .then(function(body) {
                        value = body.value;
                    });
                value;
            "#,
            &mut vm,
        )
        .unwrap();

        assert_eq!(result, JsValue::Number(0.0));
        assert_eq!(eval_script("value;", &mut vm).unwrap(), JsValue::Number(42.0));
    }

    #[test]
    fn canvas_path_records_lines_and_circles() {
        let mut attrs = std::collections::BTreeMap::new();
        attrs.insert("id".to_string(), "paint".to_string());
        let canvas = Node::element_with_attributes("canvas", attrs, vec![]);
        let root = Rc::new(RefCell::new(Node::document(vec![canvas])));
        let bridge = DomBridge::new(root);
        let mut vm = VM::new();
        bridge.attach_to_vm(&mut vm);

        eval_script(
            r##"
                let canvas = document.getElementById("paint");
                let ctx = canvas.getContext("2d");
                ctx.setStrokeStyle("#ff0000");
                ctx.setLineWidth(3);
                ctx.beginPath();
                ctx.moveTo(10, 20);
                ctx.lineTo(30, 40);
                ctx.arc(50, 60, 12, 0, 6.28318);
                ctx.stroke();
                ctx.setFillStyle("#00ff00");
                ctx.fill();
            "##,
            &mut vm,
        )
        .unwrap();

        let commands = bridge.canvas_commands_snapshot();
        let stream = commands.values().next().expect("canvas command stream");
        assert!(stream.iter().any(|command| matches!(
            command,
            wavecore_render::Canvas2DCommand::DrawLine { x1, y1, x2, y2, line_width, .. }
                if *x1 == 10.0 && *y1 == 20.0 && *x2 == 30.0 && *y2 == 40.0 && *line_width == 3.0
        )));
        assert!(stream.iter().any(|command| matches!(
            command,
            wavecore_render::Canvas2DCommand::StrokeCircle { cx, cy, radius, .. }
                if *cx == 50.0 && *cy == 60.0 && *radius == 12.0
        )));
        assert!(stream.iter().any(|command| matches!(
            command,
            wavecore_render::Canvas2DCommand::FillCircle { cx, cy, radius, .. }
                if *cx == 50.0 && *cy == 60.0 && *radius == 12.0
        )));
    }

    #[test]
    fn common_array_and_string_methods_work() {
        let mut vm = VM::new();
        let result = eval_script(
            r#"
                let values = [1, 2, 3, 4, 5];
                let filtered = values.filter(function(v) { return v > 2; });
                let reduced = filtered.reduce(function(acc, v) { return acc + v; }, 0);
                let sliced = values.slice(1, 4);
                let s = "WaveCore Browser";
                reduced
                    + sliced.length
                    + (s.startsWith("Wave") ? 1 : 0)
                    + (s.endsWith("Browser") ? 1 : 0)
                    + (s.replace("Browser", "Engine") == "WaveCore Engine" ? 1 : 0);
            "#,
            &mut vm,
        )
        .unwrap();
        assert_eq!(result, JsValue::Number(18.0));
    }

    #[test]
    fn javascript_call_depth_is_bounded() {
        let mut vm = VM::new();
        vm.max_call_depth = 32;
        let err = eval_script(
            r#"
                function recurse(n) {
                    return recurse(n + 1);
                }
                recurse(0);
            "#,
            &mut vm,
        )
        .unwrap_err();
        assert!(err.contains("maximum call stack size exceeded"));
    }

    #[test]
    fn microtask_checkpoint_has_resource_limit() {
        let mut vm = VM::new();
        vm.max_microtasks_per_checkpoint = 2;
        vm.queue_microtask(|vm| {
            vm.queue_microtask(|vm| {
                vm.queue_microtask(|_vm| Ok(()));
                Ok(())
            });
            Ok(())
        });
        let err = vm.drain_microtasks().unwrap_err();
        assert!(err.contains("microtask checkpoint exceeded configured limit"));
    }

    #[test]
    fn arraybuffer_typed_arrays_share_real_backing_store() {
        let mut vm = VM::new();
        let result = eval_script(
            r#"
                let buffer = new ArrayBuffer(8);
                let bytes = new Uint8Array(buffer);
                bytes[0] = 255;
                bytes[1] = 1;
                let words = new Uint16Array(buffer);
                let view = bytes.subarray(0, 2);
                view[1] = 2;
                buffer.byteLength + bytes.byteLength + words.length + words[0];
            "#,
            &mut vm,
        )
        .unwrap();
        assert_eq!(result, JsValue::Number(787.0));
    }

    #[test]
    fn typed_array_set_and_array_from_use_backing_values() {
        let mut vm = VM::new();
        let result = eval_script(
            r#"
                let data = new Uint16Array(4);
                data.set([10, 20, 30], 1);
                let copy = Array.from(data);
                copy[1] + copy[2] + copy[3] + data.length;
            "#,
            &mut vm,
        )
        .unwrap();
        assert_eq!(result, JsValue::Number(64.0));
    }

    #[test]
    fn eventtarget_custom_event_dispatch_and_remove_listener_work() {
        let mut attrs = std::collections::BTreeMap::new();
        attrs.insert("id".to_string(), "button".to_string());
        let button = Node::element_with_attributes("button", attrs, vec![]);
        let root = Rc::new(RefCell::new(Node::document(vec![button])));
        let bridge = DomBridge::new(root);
        let mut vm = VM::new();
        bridge.attach_to_vm(&mut vm);

        let result = eval_script(
            r#"
                let button = document.getElementById("button");
                let observed = 0;
                let handler = function(event) {
                    observed = event.detail;
                    event.preventDefault();
                };
                button.addEventListener("wave", handler);
                let first = button.dispatchEvent(
                    new CustomEvent("wave", { detail: 7, cancelable: true })
                );
                button.removeEventListener("wave", handler);
                button.dispatchEvent(new CustomEvent("wave", { detail: 9 }));
                observed + (first ? 100 : 0);
            "#,
            &mut vm,
        )
        .unwrap();

        assert_eq!(result, JsValue::Number(7.0));
    }

    #[test]
    fn fetch_data_model_headers_request_response_and_arraybuffer_work() {
        let root = Rc::new(RefCell::new(Node::document(vec![])));
        let bridge = DomBridge::with_url(root, "https://wavecore.local/");
        let mut vm = VM::new();
        bridge.attach_to_vm(&mut vm);

        eval_script(
            r#"
                let headers = new Headers({ "x-wave": "core" });
                headers.append("x-wave", "engine");
                let request = new Request("data:application/json,{\"ok\":true}", {
                    method: "GET",
                    headers: headers
                });
                let fetchedStatus = 0;
                let parsedOk = false;
                fetch(request)
                    .then(function(response) {
                        fetchedStatus = response.status;
                        return response.json();
                    })
                    .then(function(body) {
                        parsedOk = body.ok;
                    });

                let response = new Response("abc", {
                    status: 201,
                    headers: { "content-type": "text/plain" }
                });
                let responseStatus = response.status;
                let byteLength = 0;
                response.arrayBuffer().then(function(buffer) {
                    byteLength = buffer.byteLength;
                });
            "#,
            &mut vm,
        )
        .unwrap();

        let result = eval_script(
            r#"fetchedStatus + (parsedOk ? 10 : 0) + responseStatus + byteLength + (headers.get("x-wave") == "core, engine" ? 1 : 0);"#,
            &mut vm,
        )
        .unwrap();
        assert_eq!(result, JsValue::Number(415.0));
    }

    #[test]
    fn date_and_regexp_runtime_primitives_work() {
        let mut vm = VM::new();
        let result = eval_script(
            r#"
                let date = new Date(1234);
                let re = new RegExp("^wave(core)?$", "i");
                let match = re.exec("WaveCore");
                date.getTime() + (re.test("wave") ? 1 : 0) + match.length;
            "#,
            &mut vm,
        )
        .unwrap();
        assert_eq!(result, JsValue::Number(1237.0));
    }

    #[test]
    fn class_constructor_and_instance_methods_work() {
        let mut vm = VM::new();
        let result = eval_script(
            r#"
                class Counter {
                    constructor(start) {
                        this.value = start;
                    }
                    inc(step) {
                        this.value = this.value + step;
                        return this.value;
                    }
                }

                let counter = new Counter(4);
                counter.inc(3) + counter.value;
            "#,
            &mut vm,
        )
        .unwrap();

        assert_eq!(result, JsValue::Number(14.0));
    }

    #[test]
    fn mutation_observer_receives_attribute_and_text_records_as_microtasks() {
        let mut attrs = std::collections::BTreeMap::new();
        attrs.insert("id".to_string(), "target".to_string());
        let target = Node::element_with_attributes("div", attrs, vec![Node::text("before")]);
        let root = Rc::new(RefCell::new(Node::document(vec![target])));
        let bridge = DomBridge::new(root);
        let mut vm = VM::new();
        bridge.attach_to_vm(&mut vm);

        let first = eval_script(
            r#"
                let target = document.getElementById("target");
                let observed = "";
                let observer = new MutationObserver(function(records) {
                    observed = observed + records[0].type + ";";
                });
                observer.observe(target, {
                    attributes: true,
                    characterData: true
                });
                target.setAttribute("data-ready", "yes");
                target.setInnerText("after");
                observed;
            "#,
            &mut vm,
        )
        .unwrap();

        assert_eq!(first, JsValue::String(String::new()));
        assert_eq!(
            eval_script("observed;", &mut vm).unwrap(),
            JsValue::String("attributes;characterData;".to_string())
        );
    }

    #[test]
    fn async_function_and_await_resolved_promise_work() {
        let mut vm = VM::new();
        let result = eval_script(
            r#"
                async function addLater(a, b) {
                    let value = await Promise.resolve(a + b);
                    return value * 2;
                }
                let observed = 0;
                addLater(4, 5).then(function(value) {
                    observed = value;
                });
                observed;
            "#,
            &mut vm,
        )
        .unwrap();

        assert_eq!(result, JsValue::Number(0.0));
        assert_eq!(
            eval_script("observed;", &mut vm).unwrap(),
            JsValue::Number(18.0)
        );
    }

    #[test]
    fn intl_number_date_and_collator_surface_work() {
        let mut vm = VM::new();
        let result = eval_script(
            r#"
                let nf = new Intl.NumberFormat("en-US");
                let df = new Intl.DateTimeFormat("th-TH");
                let collator = new Intl.Collator("en-US");
                let number = nf.format(1234.5);
                let date = df.format(new Date(5000));
                (number == "1234.5" ? 1 : 0)
                    + (date == "5000" ? 2 : 0)
                    + (collator.compare("a", "b") < 0 ? 4 : 0);
            "#,
            &mut vm,
        )
        .unwrap();

        assert_eq!(result, JsValue::Number(7.0));
    }

}
