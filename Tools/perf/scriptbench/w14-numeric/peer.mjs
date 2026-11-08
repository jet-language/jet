let total = 0;
for (let i = 1; i <= 30000000; i++) total += (i * i) % 7919;
console.log(total);
