type Status = "pending" | "done" | "paused";
function label(status: Status): string {
    switch (status) {
        case "pending": return "pending";
        case "done": return "done";
        default: { const unreachable: never = status; return unreachable; }
    }
}
console.log(label("pending"));
