let count = 0;

const display = () => `count: ${count}`;
const increment = () => { count += 1; };
const reset = () => { count = 0; };

for (let i = 0; i < 3; i += 1) increment();
console.log(display());
reset();
console.log(display());
