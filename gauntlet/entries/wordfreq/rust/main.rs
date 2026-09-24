use std::{collections::HashMap, env, fs};

fn main() {
    let input = fs::read_to_string(env::args().nth(1).unwrap()).unwrap();
    let mut counts = HashMap::<&str, usize>::new();
    let mut total: usize = 0;
    for word in input
        .split(|character: char| character == ' ' || ('\t'..='\r').contains(&character))
        .filter(|word| !word.is_empty())
    {
        *counts.entry(word).or_default() += 1;
        total += 1;
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by(|(left_word, left_count), (right_word, right_count)| {
        right_count.cmp(left_count).then_with(|| left_word.cmp(right_word))
    });
    for (word, count) in ranked.iter().take(20) {
        println!("{count} {word}");
    }
    println!("distinct {} total {}", ranked.len(), total);
}
