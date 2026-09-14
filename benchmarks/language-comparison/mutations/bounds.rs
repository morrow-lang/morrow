fn main() {
    let values = vec![1_i64];
    let index = std::env::args().len() + 2;
    println!("{}", values[index]);
}
