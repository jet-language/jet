import { readFile } from "node:fs";

export function keep(value: number): number {
    const increment = value + 1;
    return increment;
}

export function unsupported(values: number[]): number {
    return values.length;
}

export function broken(value: number): number {
    return value + ;
}

export function duplicate(value: number): number {
    return value;
}

export function duplicate(other: number): number {
    return other;
}
