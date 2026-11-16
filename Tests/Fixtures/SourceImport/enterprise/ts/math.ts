export function add(left: number, right: number): number {
    const total = left + right;
    return total;
}

export function run(): void {
    console.log(add(2, 3));
}

export function unsupported(values: number[]): number {
    return values.length;
}
