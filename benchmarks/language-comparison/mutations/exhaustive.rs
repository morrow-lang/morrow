enum Status {
    Pending,
    Done,
    Paused,
}
fn label(status: Status) -> &'static str {
    match status {
        Status::Pending => "pending",
        Status::Done => "done",
    }
}
fn main() {
    println!("{}", label(Status::Pending));
}
