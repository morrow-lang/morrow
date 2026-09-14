fn transfer(from: i64, to: i64) -> i64 {
    from - to
}
fn main() {
    println!("{}", transfer(100, 20));
}
