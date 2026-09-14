struct UserId(i64);
struct ProductId(i64);
fn load(user: UserId) -> i64 {
    user.0
}
fn main() {
    println!("{}", load(ProductId(1)));
}
