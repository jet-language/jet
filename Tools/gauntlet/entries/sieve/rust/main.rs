use std::env;

fn sieve(n: usize) -> Vec<u8> {
    let mut prime = vec![1u8; n];
    if n > 0 {
        prime[0] = 0;
    }
    if n > 1 {
        prime[1] = 0;
    }
    let mut p = 3;
    while p * p < n {
        if prime[p] == 1 {
            let mut multiple = p * p;
            while multiple < n {
                prime[multiple] = 0;
                multiple += p * 2;
            }
        }
        p += 2;
    }
    prime
}

fn main() {
    let n: usize = env::args().nth(1).unwrap().parse().unwrap();
    let prime = sieve(n);
    let mut count = 0;
    let mut largest = 0;
    if n > 2 {
        count = 1;
        largest = 2;
        for i in (3..n).step_by(2) {
            if prime[i] == 1 {
                count += 1;
                largest = i;
            }
        }
    }
    println!("count {count}");
    println!("largest {largest}");
}
