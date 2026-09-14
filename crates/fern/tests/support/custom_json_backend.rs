use super::*;

fn compiled(source: &str) -> Program {
    let checked =
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    fern_compiler::lowering::lower(&checked).unwrap()
}

const MAIN: &str = r#"unsafe extern "C" { fn fern_main() -> i32; }
fn main() { assert_eq!(unsafe { fern_main() }, 0); }
"#;

#[test]
fn custom_json_native_float_bool_and_generic_callbacks_survive_collection() {
    let source = r#"newtype Decimal = Decimal(Float)
impl Json(Decimal):
    fn to_json(value: Decimal) -> Result(json.Value,json.Error): json.from_float(value.0)
    fn from_json(value: json.Value) -> Result(Decimal,json.Error): Ok(Decimal(json.as_float(value)?))
newtype Flag = Flag(Bool)
impl Json(Flag):
    fn to_json(value: Flag) -> Result(json.Value,json.Error): Ok(json.from_bool(value.0))
    fn from_json(value: json.Value) -> Result(Flag,json.Error): Ok(Flag(json.as_bool(value)?))
newtype Box(a) = Box(a)
impl Json(Box(a)) where Json(a):
    fn to_json(value: Box(a)) -> Result(json.Value,json.Error): to_json(value.0)
    fn from_json(value: json.Value) -> Result(Box(a),json.Error): Ok(Box(from_json(value)?))
fn main() -> Result((),json.Error):
    println(json.encode((Decimal(-1234.5),Flag(true),Flag(false)))?)
    let decoded = json.decode("[-1234.5,true,false]",(Decimal,Flag,Flag))?
    println(decoded.0.0 == -1234.5)
    println(decoded.1.0)
    println(decoded.2.0)
    println(json.encode(Box(9007199254740993))?)
    let boxed = json.decode("9223372036854775807",Box(Int))?
    println(boxed.0)
    Ok(())
"#;
    let mut program = compiled(source);
    let mut inserted = 0;
    for function in &mut program.functions {
        let mut body = Vec::new();
        for statement in function.body.drain(..) {
            if matches!(&statement, Statement::Assign { operation: Operation::Call { callee: Operand::Symbol(name), .. }, .. } if matches!(machine::bare(name), "fern_json_value_as_float" | "fern_json_value_as_bool" | "fern_json_value_as_int" | "fern_json_codec_decode"))
            {
                body.push(Statement::Effect(Operation::Call {
                    callee: symbol("fern_gc_collect_precise"),
                    args: vec![],
                    variadic: None,
                }));
                inserted += 1;
            }
            body.push(statement);
        }
        function.body = body;
    }
    assert!(
        inserted >= 3,
        "exercise each concrete callback representation"
    );
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            MAIN,
            &[core_runtime_archive().into_os_string()]
        ),
        b"[-1234.5,true,false]\ntrue\ntrue\nfalse\n9007199254740993\n9223372036854775807\n"
    );
}

#[test]
fn custom_json_native_constructor_quota_unwinds_and_recursion_is_bounded() {
    let source = r#"newtype Quota = Quota(Int)
impl Json(Quota):
    fn to_json(value: Quota) -> Result(json.Value,json.Error):
        defer println("quota cleanup")
        for index in 0..value.0:
            let ignored = json.from_int(index)
            ()
        Ok(json.null())
    fn from_json(value: json.Value) -> Result(Quota,json.Error): Ok(Quota(json.as_int(value)?))
newtype Recursive = Recursive(Int)
impl Json(Recursive):
    fn to_json(value: Recursive) -> Result(json.Value,json.Error): json.parse(json.encode(value)?)
    fn from_json(value: json.Value) -> Result(Recursive,json.Error): Ok(Recursive(json.as_int(value)?))
fn main():
    match json.encode(Quota(100001)):
        Ok(_) -> println(-1)
        Err(error) -> println(json.error_code(error))
    match json.encode(Recursive(1)):
        Ok(_) -> println(-2)
        Err(error) -> println(json.error_code(error))
    match json.encode(Quota(1)):
        Ok(text) -> println(text)
        Err(error) -> println(json.error_code(error))
"#;
    assert_eq!(
        NativeFixture::new().execute_linked(
            &compiled(source),
            MAIN,
            &[core_runtime_archive().into_os_string()]
        ),
        b"quota cleanup\n4\n4\nquota cleanup\nnull\n"
    );
}

#[test]
fn custom_json_native_callback_fault_preserves_the_original_failure_and_cleanup() {
    let source = r#"newtype Broken = Broken(Int)
impl Json(Broken):
    fn to_json(value: Broken) -> Result(json.Value,json.Error):
        defer println("callback cleanup")
        Ok(json.from_int(1 / value.0))
    fn from_json(value: json.Value) -> Result(Broken,json.Error): Ok(Broken(json.as_int(value)?))
fn probe(value: Int) -> Int:
    defer println("caller cleanup")
    match json.encode(Broken(value)):
        Ok(_) -> 1
        Err(_) -> 2
fn main(): ()
"#;
    let checked =
        fern_compiler::check::check(&fern_compiler::parse::parse(source).unwrap()).unwrap();
    let id = checked
        .functions
        .iter()
        .find(|f| f.name == "probe")
        .unwrap()
        .id
        .0;
    let mut program = fern_compiler::lowering::lower(&checked).unwrap();
    program
        .functions
        .iter_mut()
        .find(|f| machine::bare(&f.name) == format!("f{id}"))
        .unwrap()
        .export = true;
    let harness = format!(
        r#"unsafe extern "C" {{ fn f{id}(env:usize,fault:*mut i64,value:i64)->i64; }}
fn main() {{ let mut fault=0; unsafe {{ f{id}(0,&mut fault,0); }} assert_eq!(fault,1); println!("original division fault"); }}"#
    );
    assert_eq!(
        NativeFixture::new().execute_linked(
            &program,
            &harness,
            &[core_runtime_archive().into_os_string()]
        ),
        b"callback cleanup\ncaller cleanup\noriginal division fault\n"
    );
}

#[test]
fn custom_json_native_nested_error_paths_compose_without_losing_the_code() {
    let source = r#"newtype Path = Path(Int)
impl Json(Path):
    fn to_json(value: Path) -> Result(json.Value,json.Error): Ok(json.from_int(value.0))
    fn from_json(value: json.Value) -> Result(Path,json.Error):
        let data=json.decode("\{\"inside\":[\"bad\"]\}",Map(String,List(Int)))?
        Ok(Path(Map.len(data)))
type Paths derive(Json):
    values: List(Path)
fn main():
    match json.decode("\{\"values\":[null]\}",Paths):
        Ok(_) -> println(0)
        Err(error) ->
            println(json.error_code(error))
            println(json.error_path(error))
"#;
    assert_eq!(
        NativeFixture::new().execute_linked(
            &compiled(source),
            MAIN,
            &[core_runtime_archive().into_os_string()]
        ),
        b"5\n/values/0/inside/0\n"
    );
}
