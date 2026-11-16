#[derive(Clone)]
struct Module {
    id: usize,
    deps: &'static [usize],
}

fn invalidated(modules: &[Module], changed: usize) -> Vec<usize> {
    let mut dirty = vec![changed];
    for module in modules {
        if dirty.contains(&module.id) {
            continue;
        }
        if module.deps.iter().any(|dependency| dirty.contains(dependency)) {
            dirty.push(module.id);
        }
    }
    dirty.sort_unstable();
    dirty
}

fn main() {
    let modules = [
        Module { id: 0, deps: &[] },
        Module { id: 1, deps: &[0] },
        Module { id: 2, deps: &[1] },
        Module { id: 3, deps: &[2] },
        Module { id: 4, deps: &[] },
    ];
    let dirty = invalidated(&modules, 1);
    assert_eq!(dirty, [1, 2, 3]);
    println!("changed=ast invalidated=ast,sema,codegen retained=parse,docs");
    println!("cache=source+interface ownership=graph-owned");
}
