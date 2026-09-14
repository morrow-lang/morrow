fn validate(value: i64) -> Result<i64, &'static str> {
    if value > 0 {
        Ok(value)
    } else {
        Err("positive value required")
    }
}
fn main() {
    validate(-1);
    println!("0");
}
