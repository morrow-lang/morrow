// Bun strips types; the companion tsc check is a separate operation.
declare const process: { argv: string[] };
type Cell = Readonly<{ index: number; value: number }>;

function scalar(steps: number, value: number): number {
    for (let step = 0; step < steps; step++) value = (value * 48271) % 2147483647;
    return value;
}

function scalarBigInt(steps: number, value: bigint): bigint {
    for (let step = 0; step < steps; step++) value = (value * 48271n) % 2147483647n;
    return value;
}

function model(steps: number, seed: number, mutable: boolean): [number, number] {
    const original: readonly Cell[] = Array.from({ length: 256 }, (_, index) => ({ index, value: 0 }));
    let values = [...original];
    for (let step = 0; step < steps; step++) {
        const target = (seed + step * 17) % 256;
        if (mutable) {
            const cell = values[target];
            if (!cell) throw new Error("controlled index outside model");
            values[target] = { ...cell, value: cell.value + 1 };
        } else {
            values = values.map(cell => cell.index === target ? { ...cell, value: cell.value + 1 } : cell);
        }
    }
    return [values.reduce((sum, cell) => sum + (cell.index + 1) * cell.value, 0),
        original.reduce((sum, cell) => sum + cell.value, 0)];
}

const steps = Number(process.argv[3]);
const seed = Number(process.argv[4]);
switch (process.argv[2]) {
    case "scalar": console.log(scalar(steps, seed)); break;
    case "scalar-bigint": console.log(scalarBigInt(steps, BigInt(seed)).toString()); break;
    case "precision": console.log((9007199254740993n + BigInt(seed)).toString()); break;
    case "precision-number": console.log(9007199254740993 + seed); break;
    default: {
        const [sum, old] = model(steps, seed, process.argv[2] === "mutable");
        console.log(`${sum}\n${old}`);
    }
}
