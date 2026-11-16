const values = Array.from({ length: 1024 }, (_, value) => value * value);
console.log(`${values[0]}:${values[1023]}`);
