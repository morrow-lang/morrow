type Result<T> = { ok: true; value: T } | { ok: false; error: string };
function validate(value: number): Result<number> {
    return value > 0 ? { ok: true, value } : { ok: false, error: "positive value required" };
}
validate(-1);
console.log(0);
