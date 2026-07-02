mod jet_layout {
    // D-LAYOUT1 / D-LAYOUT-GATES1 (ratified 2026-06-28/29): a Cassowary-style
    // linear constraint solver backing `layout NAME { … }`. I6: zero external
    // crates — a hand-rolled two-phase primal simplex, plain std Rust only.
    //
    // Design (the sema/codegen half lives in Sema/CheckerCoreLib.rs,
    // Sema/CheckerInfer/{binary,calls}.rs, Codegen/TIR/{mod,lower,emit}.rs):
    //   * `HVar`/`VVar`/`LengthVar` all erase to `LinExpr` here — the
    //     horizontal/vertical/neutral axis distinction is checked entirely at
    //     compile time (GATE 1/2); there is nothing to represent at runtime.
    //   * A `layout NAME { … }` block's `box.anchor` reads desugar (parser,
    //     D-LAYOUT1) to `NAME.h(box, anchor)` / `NAME.v(box, anchor)` — both
    //     call the SAME internal `var()`, since the axis distinction is erased.
    //   * `>=`/`<=`/`==` between layout values (GATE 1) lower to the free
    //     functions `ge`/`le`/`eq_` below (Rust operator overloading can't
    //     return a non-`bool` type via `>=`/`==` syntax).
    //   * Layout values are assumed NON-NEGATIVE (widths, positions, gaps —
    //     the universal case for UI layout; a signed-variable extension via
    //     the standard `x = x+ - x-` split is a contained follow-up, not
    //     needed for the ratified surface).
    //   * Every constraint is REQUIRED by default; `.weak()`/`.medium()`/
    //     `.strong()` (Cassowary-style priorities, collapsed to one weighted
    //     objective rather than the paper's lexicographic objective stack —
    //     see `Strength::weight`) turn it into a soft goal a conflicting
    //     required constraint always wins against. Soft constraints use the
    //     classic Cassowary "error variable" technique, which doubles as the
    //     row's phase-1-feasible seed (no artificial ever needed for a soft
    //     row); required rows use a standard two-phase artificial-variable
    //     seed. This is a real, from-scratch re-solve on every `.add()`/
    //     `.suggest()` (not Cassowary's incremental dual-simplex update) —
    //     correct and fine at UI scale; true incrementality is a performance
    //     follow-up, not a correctness requirement.
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    const EPS: f64 = 1e-7;

    // ---------------------------------------------------------------------
    // Linear expressions — what `HVar`/`VVar`/`LengthVar` erase to.
    // ---------------------------------------------------------------------

    /// Max distinct box-anchor variables one constraint EXPRESSION (one side
    /// of a `>=`/`<=`/`==`, built by chaining `+`/`-`) can reference. Real
    /// layout lines stay well under this (2-4 terms); generous headroom, not
    /// a hard language limit — `combine` panics with a clear message if it's
    /// ever exceeded (would mean a genuinely unusual expression, worth
    /// surfacing loudly rather than silently truncating).
    const MAX_TERMS: usize = 16;

    /// `LinExpr` is what `HVar`/`VVar`/`LengthVar` erase to — and it needs to
    /// be `Copy`: a `layout {}` line routinely reuses a captured variable
    /// (`xv #= form.h("x","width"); form.suggest(xv, 100.0); form.value(xv)`),
    /// and Jet's general non-Copy-reuse clone-insertion doesn't (yet) know
    /// about this closed type family, so a `Vec`/`Rc`-backed representation
    /// would hit a real "use of moved value" in the generated Rust the first
    /// time a variable was captured and used twice. A fixed-size term array
    /// (no `Vec`) plus a `u32` handle INDEX into a thread-local registry (not
    /// an `Rc` directly — `Rc` isn't `Copy`) keeps every field `Copy`.
    #[derive(Clone, Copy)]
    pub struct LinExpr {
        terms: [(usize, f64); MAX_TERMS],
        n_terms: u8,
        constant: f64,
        handle: Option<u32>,
    }

    thread_local! {
        /// The handle registry `LinExpr` indexes into (see `MAX_TERMS` doc).
        /// Handles are never removed — a `layout {}` block's solver state
        /// lives for the rest of the program, same as any other value.
        static HANDLE_REGISTRY: RefCell<Vec<Rc<RefCell<Inner>>>> = RefCell::new(Vec::new());
    }

    fn register_handle(inner: Rc<RefCell<Inner>>) -> u32 {
        HANDLE_REGISTRY.with(|r| {
            let mut r = r.borrow_mut();
            r.push(inner);
            (r.len() - 1) as u32
        })
    }

    fn handle_by_id(id: u32) -> Rc<RefCell<Inner>> {
        HANDLE_REGISTRY.with(|r| r.borrow()[id as usize].clone())
    }

    impl LinExpr {
        /// A plain number used where a layout value is expected (a bare
        /// `Int`/`Float` operand — axis-neutral, elaborates to `LengthVar`).
        pub fn from_const(c: f64) -> LinExpr {
            LinExpr {
                terms: [(0, 0.0); MAX_TERMS],
                n_terms: 0,
                constant: c,
                handle: None,
            }
        }

        fn from_var(idx: usize, handle_id: u32) -> LinExpr {
            let mut terms = [(0, 0.0); MAX_TERMS];
            terms[0] = (idx, 1.0);
            LinExpr {
                terms,
                n_terms: 1,
                constant: 0.0,
                handle: Some(handle_id),
            }
        }

        fn combine(&self, other: &LinExpr, sign: f64) -> LinExpr {
            let mut terms = self.terms;
            let mut n = self.n_terms as usize;
            for &(idx, coeff) in &other.terms[..other.n_terms as usize] {
                if let Some(slot) = terms[..n].iter_mut().find(|(i, _)| *i == idx) {
                    slot.1 += sign * coeff;
                } else {
                    assert!(
                        n < MAX_TERMS,
                        "layout constraint expression has more than {MAX_TERMS} distinct terms"
                    );
                    terms[n] = (idx, sign * coeff);
                    n += 1;
                }
            }
            LinExpr {
                terms,
                n_terms: n as u8,
                constant: self.constant + sign * other.constant,
                handle: self.handle.or(other.handle),
            }
        }

        fn eval(&self, solution: &[f64]) -> f64 {
            let mut v = self.constant;
            for &(idx, coeff) in &self.terms[..self.n_terms as usize] {
                v += coeff * solution.get(idx).copied().unwrap_or(0.0);
            }
            v
        }

        fn terms(&self) -> &[(usize, f64)] {
            &self.terms[..self.n_terms as usize]
        }
    }

    // `codegen` emits plain `(lhs) + (rhs)` for a layout `+`/`-` (mirroring
    // D-SIMD2's element-wise operators) — the operand ownership shape it
    // renders isn't something this prelude module controls, so all four
    // owned/borrowed combinations are provided.
    impl From<&LinExpr> for LinExpr {
        fn from(v: &LinExpr) -> LinExpr {
            v.clone()
        }
    }

    impl std::ops::Add for LinExpr {
        type Output = LinExpr;
        fn add(self, rhs: LinExpr) -> LinExpr {
            self.combine(&rhs, 1.0)
        }
    }
    impl std::ops::Add<&LinExpr> for LinExpr {
        type Output = LinExpr;
        fn add(self, rhs: &LinExpr) -> LinExpr {
            self.combine(rhs, 1.0)
        }
    }
    impl std::ops::Add<LinExpr> for &LinExpr {
        type Output = LinExpr;
        fn add(self, rhs: LinExpr) -> LinExpr {
            self.combine(&rhs, 1.0)
        }
    }
    impl std::ops::Add<&LinExpr> for &LinExpr {
        type Output = LinExpr;
        fn add(self, rhs: &LinExpr) -> LinExpr {
            self.combine(rhs, 1.0)
        }
    }

    impl std::ops::Sub for LinExpr {
        type Output = LinExpr;
        fn sub(self, rhs: LinExpr) -> LinExpr {
            self.combine(&rhs, -1.0)
        }
    }
    impl std::ops::Sub<&LinExpr> for LinExpr {
        type Output = LinExpr;
        fn sub(self, rhs: &LinExpr) -> LinExpr {
            self.combine(rhs, -1.0)
        }
    }
    impl std::ops::Sub<LinExpr> for &LinExpr {
        type Output = LinExpr;
        fn sub(self, rhs: LinExpr) -> LinExpr {
            self.combine(&rhs, -1.0)
        }
    }
    impl std::ops::Sub<&LinExpr> for &LinExpr {
        type Output = LinExpr;
        fn sub(self, rhs: &LinExpr) -> LinExpr {
            self.combine(rhs, -1.0)
        }
    }

    // ---------------------------------------------------------------------
    // Constraint rows
    // ---------------------------------------------------------------------

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum RelOp {
        Le,
        Ge,
        Eq,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Strength {
        Required,
        Strong,
        Medium,
        Weak,
    }

    impl Strength {
        fn weight(self) -> f64 {
            match self {
                Strength::Required => 0.0, // unused — required rows never carry an error column
                Strength::Strong => 1_000_000.0,
                Strength::Medium => 1_000.0,
                Strength::Weak => 1.0,
            }
        }
    }

    #[derive(Clone)]
    struct RowSpec {
        terms: Vec<(usize, f64)>,
        rhs: f64,
        op: RelOp,
        strength: Strength,
        label: String,
    }

    /// Normalize so `rhs >= 0` (flip the row and the op otherwise) — every
    /// downstream column allocation assumes a non-negative RHS.
    fn normalize_row(mut row: RowSpec) -> RowSpec {
        if row.rhs < 0.0 {
            row.rhs = -row.rhs;
            for t in row.terms.iter_mut() {
                t.1 = -t.1;
            }
            row.op = match row.op {
                RelOp::Le => RelOp::Ge,
                RelOp::Ge => RelOp::Le,
                RelOp::Eq => RelOp::Eq,
            };
        }
        row
    }

    fn build_row(lhs: &LinExpr, rhs: &LinExpr, op: RelOp, strength: Strength, label: String) -> RowSpec {
        // `lhs OP rhs` → `(lhs - rhs) OP 0` → `terms · x OP (-constant)`.
        let combined = lhs.combine(rhs, -1.0);
        normalize_row(RowSpec {
            terms: combined.terms().to_vec(),
            rhs: -combined.constant,
            op,
            strength,
            label,
        })
    }

    // ---------------------------------------------------------------------
    // The solver
    // ---------------------------------------------------------------------

    pub struct Inner {
        #[allow(dead_code)]
        name: String,
        boxes: HashMap<String, HashMap<String, usize>>,
        var_names: Vec<String>,
        rows: Vec<RowSpec>,
        edits: HashMap<usize, f64>,
        solution: Vec<f64>,
        dirty: bool,
        infeasible: Option<Vec<String>>,
    }

    /// One row of the dense simplex tableau: `n_cols` coefficients + RHS.
    struct Tableau {
        n_cols: usize,
        rows: Vec<Vec<f64>>, // each of length n_cols + 1 (last slot = RHS)
        basis: Vec<usize>,
    }

    impl Tableau {
        /// Minimize `cost` (length `n_cols`) over this tableau, Bland's rule
        /// throughout (guarantees termination on these small problems).
        /// `ineligible[j]` columns are never chosen as the entering variable
        /// (used to keep phase-1 artificials out of the phase-2 basis).
        fn minimize(&mut self, cost: &[f64], ineligible: &[bool]) -> f64 {
            let n = self.n_cols;
            let m = self.rows.len();
            let mut obj = vec![0.0f64; n + 1];
            obj[..n].copy_from_slice(cost);
            for i in 0..m {
                let bcol = self.basis[i];
                let c = cost[bcol];
                if c.abs() > EPS {
                    for k in 0..=n {
                        obj[k] -= c * self.rows[i][k];
                    }
                }
            }
            loop {
                let mut enter: Option<usize> = None;
                for j in 0..n {
                    if ineligible[j] {
                        continue;
                    }
                    if obj[j] < -EPS {
                        enter = Some(j);
                        break; // Bland's rule: first eligible negative column
                    }
                }
                let Some(j) = enter else { break };
                let mut leave: Option<usize> = None;
                let mut best_ratio = f64::INFINITY;
                for i in 0..m {
                    let a = self.rows[i][j];
                    if a > EPS {
                        let ratio = self.rows[i][n] / a;
                        let better = ratio < best_ratio - EPS;
                        let tied = (ratio - best_ratio).abs() <= EPS;
                        if better || (tied && leave.map_or(true, |l| self.basis[i] < self.basis[l])) {
                            best_ratio = ratio;
                            leave = Some(i);
                        }
                    }
                }
                let Some(i) = leave else { break }; // unbounded — shouldn't happen for this problem family
                let piv = self.rows[i][j];
                for v in self.rows[i].iter_mut() {
                    *v /= piv;
                }
                for k in 0..m {
                    if k == i {
                        continue;
                    }
                    let factor = self.rows[k][j];
                    if factor.abs() > EPS {
                        for c in 0..=n {
                            let rv = self.rows[i][c];
                            self.rows[k][c] -= factor * rv;
                        }
                    }
                }
                let factor = obj[j];
                if factor.abs() > EPS {
                    for c in 0..=n {
                        let rv = self.rows[i][c];
                        obj[c] -= factor * rv;
                    }
                }
                self.basis[i] = j;
            }
            -obj[n]
        }
    }

    /// Column kinds allocated per row, beyond the real (box-anchor) vars.
    enum Extra {
        /// Required `<=`: one slack, cost 0 always, initial basis.
        Slack { col: usize },
        /// Required `>=`: surplus (cost 0) + artificial (phase-1 cost 1,
        /// phase-2 ineligible), artificial is initial basis.
        SurplusArtificial { surplus: usize, artificial: usize },
        /// Required `==`: one artificial, initial basis.
        Artificial { col: usize },
        /// Soft (any op): error pair, `e_minus` is initial basis. Exactly one
        /// of the pair carries the row's strength weight in phase 2,
        /// depending on the original op (`Eq` weights both).
        ErrorPair { e_plus: usize, e_minus: usize },
    }

    fn solve(inner: &mut Inner) {
        let n_vars = inner.var_names.len();
        let mut specs: Vec<RowSpec> = inner.rows.clone();
        for (&idx, &val) in inner.edits.iter() {
            specs.push(normalize_row(RowSpec {
                terms: vec![(idx, 1.0)],
                rhs: val,
                op: RelOp::Eq,
                strength: Strength::Medium,
                label: format!("suggest({})", inner.var_names.get(idx).cloned().unwrap_or_default()),
            }));
        }
        if specs.is_empty() {
            inner.solution = vec![0.0; n_vars];
            inner.infeasible = None;
            return;
        }
        // Allocate extra columns.
        let mut next_col = n_vars;
        let mut extras: Vec<Extra> = Vec::with_capacity(specs.len());
        for s in &specs {
            let extra = if s.strength == Strength::Required {
                match s.op {
                    RelOp::Le => {
                        let col = next_col;
                        next_col += 1;
                        Extra::Slack { col }
                    }
                    RelOp::Ge => {
                        let surplus = next_col;
                        let artificial = next_col + 1;
                        next_col += 2;
                        Extra::SurplusArtificial { surplus, artificial }
                    }
                    RelOp::Eq => {
                        let col = next_col;
                        next_col += 1;
                        Extra::Artificial { col }
                    }
                }
            } else {
                let e_plus = next_col;
                let e_minus = next_col + 1;
                next_col += 2;
                Extra::ErrorPair { e_plus, e_minus }
            };
            extras.push(extra);
        }
        let n_cols = next_col;
        let m = specs.len();
        let mut rows = vec![vec![0.0f64; n_cols + 1]; m];
        let mut basis = vec![0usize; m];
        let mut has_artificial = vec![false; m];
        for (i, (s, extra)) in specs.iter().zip(extras.iter()).enumerate() {
            for &(idx, coeff) in &s.terms {
                rows[i][idx] += coeff;
            }
            rows[i][n_cols] = s.rhs;
            match *extra {
                Extra::Slack { col } => {
                    rows[i][col] = 1.0;
                    basis[i] = col;
                }
                Extra::SurplusArtificial { surplus, artificial } => {
                    rows[i][surplus] = -1.0;
                    rows[i][artificial] = 1.0;
                    basis[i] = artificial;
                    has_artificial[i] = true;
                }
                Extra::Artificial { col } => {
                    rows[i][col] = 1.0;
                    basis[i] = col;
                    has_artificial[i] = true;
                }
                Extra::ErrorPair { e_plus, e_minus } => {
                    rows[i][e_plus] = -1.0;
                    rows[i][e_minus] = 1.0;
                    basis[i] = e_minus;
                }
            }
        }
        let mut tab = Tableau {
            n_cols,
            rows,
            basis,
        };
        // Phase 1: minimize the sum of the required-row artificials.
        let any_artificial = has_artificial.iter().any(|&b| b);
        if any_artificial {
            let mut cost1 = vec![0.0f64; n_cols];
            for (extra, present) in extras.iter().zip(has_artificial.iter()) {
                if !present {
                    continue;
                }
                match *extra {
                    Extra::SurplusArtificial { artificial, .. } => cost1[artificial] = 1.0,
                    Extra::Artificial { col } => cost1[col] = 1.0,
                    _ => {}
                }
            }
            let ineligible1 = vec![false; n_cols]; // nothing barred in phase 1
            let phase1_obj = tab.minimize(&cost1, &ineligible1);
            if phase1_obj > 1e-5 {
                // Infeasible: report every required row whose artificial is
                // still basic with a positive value — a real, if imperfect,
                // conflict trace straight from the simplex tableau.
                let mut names = Vec::new();
                for (i, extra) in extras.iter().enumerate() {
                    let art_col = match *extra {
                        Extra::SurplusArtificial { artificial, .. } => Some(artificial),
                        Extra::Artificial { col } => Some(col),
                        _ => None,
                    };
                    if let Some(art) = art_col {
                        if tab.basis[i] == art && tab.rows[i][n_cols] > EPS {
                            names.push(specs[i].label.clone());
                        }
                    }
                }
                if names.is_empty() {
                    names.push("(layout has no feasible solution)".to_string());
                }
                inner.infeasible = Some(names);
                inner.solution = vec![0.0; n_vars];
                return;
            }
        }
        // Phase 2: minimize the weighted sum of soft-constraint violations.
        // Artificials are barred from re-entering the basis.
        let mut cost2 = vec![0.0f64; n_cols];
        let mut ineligible2 = vec![false; n_cols];
        for (extra, s) in extras.iter().zip(specs.iter()) {
            match *extra {
                Extra::SurplusArtificial { artificial, .. } => ineligible2[artificial] = true,
                Extra::Artificial { col } => ineligible2[col] = true,
                Extra::ErrorPair { e_plus, e_minus } => {
                    let w = s.strength.weight();
                    match s.op {
                        RelOp::Eq => {
                            cost2[e_plus] = w;
                            cost2[e_minus] = w;
                        }
                        RelOp::Le => cost2[e_plus] = w,
                        RelOp::Ge => cost2[e_minus] = w,
                    }
                }
                Extra::Slack { .. } => {}
            }
        }
        tab.minimize(&cost2, &ineligible2);
        let mut solution = vec![0.0f64; n_vars];
        for i in 0..m {
            if tab.basis[i] < n_vars {
                solution[tab.basis[i]] = tab.rows[i][n_cols];
            }
        }
        inner.infeasible = None;
        inner.solution = solution;
    }

    // ---------------------------------------------------------------------
    // Public handle / constraint API — every method name here IS the Jet
    // method name (`Codegen/TIR/mod.rs` `THandleOp::LayoutMethod` is a pure
    // passthrough).
    // ---------------------------------------------------------------------

    #[derive(Clone)]
    pub struct Handle(Rc<RefCell<Inner>>, u32);

    impl Handle {
        pub fn new(name: &str) -> Handle {
            let inner = Rc::new(RefCell::new(Inner {
                name: name.to_string(),
                boxes: HashMap::new(),
                var_names: Vec::new(),
                rows: Vec::new(),
                edits: HashMap::new(),
                solution: Vec::new(),
                dirty: false,
                infeasible: None,
            }));
            let id = register_handle(inner.clone());
            Handle(inner, id)
        }

        // `box_name`/`anchor` accept `impl AsRef<str>` (not `&str`) because
        // this prelude module doesn't control whether Jet's general
        // call-argument lowering hands a method a `String` (Read convention
        // on a non-scalar, its usual choice) or a `&str`/`&String` — tolerate
        // either.
        fn var(&self, box_name: impl AsRef<str>, anchor: impl AsRef<str>) -> LinExpr {
            let box_name = box_name.as_ref();
            let anchor = anchor.as_ref();
            let idx;
            {
                let mut inner = self.0.borrow_mut();
                let existing = inner.boxes.get(box_name).and_then(|m| m.get(anchor)).copied();
                idx = match existing {
                    Some(i) => i,
                    None => {
                        let i = inner.var_names.len();
                        inner.var_names.push(format!("{}.{}", box_name, anchor));
                        inner
                            .boxes
                            .entry(box_name.to_string())
                            .or_insert_with(HashMap::new)
                            .insert(anchor.to_string(), i);
                        inner.dirty = true;
                        i
                    }
                };
            }
            LinExpr::from_var(idx, self.1)
        }

        /// D-LAYOUT1: horizontal box anchors (`left`/`right`/`width`) — the
        /// `NAME.h(box, anchor)` a `box.anchor` read desugars to.
        pub fn h(&self, box_name: impl AsRef<str>, anchor: impl AsRef<str>) -> LinExpr {
            self.var(box_name, anchor)
        }

        /// D-LAYOUT1: vertical box anchors (`top`/`bottom`/`height`).
        pub fn v(&self, box_name: impl AsRef<str>, anchor: impl AsRef<str>) -> LinExpr {
            self.var(box_name, anchor)
        }

        /// Read the solved value of a layout variable (re-solves if dirty).
        /// `impl Into<LinExpr>` (with the `From<&LinExpr>` impl below) accepts
        /// either an owned or a borrowed argument — the call-argument
        /// ownership convention Jet's general method-call lowering picks for
        /// a bare `Infer`-convention argument isn't something this prelude
        /// module controls, so the API tolerates either shape.
        /// Panics (loudly, with the conflicting constraints named) if the
        /// layout is infeasible — a silent wrong number is a worse footgun
        /// than a clear panic (I1). Check `is_feasible()`/`conflict()` first
        /// if infeasibility is an expected, handled case rather than a bug.
        pub fn value(&self, v: impl Into<LinExpr>) -> f64 {
            self.resolve();
            let v = v.into();
            let inner = self.0.borrow();
            if let Some(conflict) = &inner.infeasible {
                // Build the whole panic message (which borrows `conflict`,
                // itself borrowed from `inner`) BEFORE dropping `inner` —
                // dropping first and using `conflict` after doesn't work,
                // `conflict` is a reference INTO `inner`'s storage.
                let msg = format!(
                    "layout \"{}\" has no feasible solution — conflicting constraints: {}",
                    inner.name,
                    conflict.join(", ")
                );
                drop(inner);
                panic!("{}", msg);
            }
            v.eval(&inner.solution)
        }

        /// Cassowary "edit variable": suggest a preferred value for a single
        /// layout variable without adding a permanent required constraint.
        /// Calling it again on the same variable REPLACES the previous
        /// suggestion (keyed by variable index, not accumulated).
        pub fn suggest(&self, v: impl Into<LinExpr>, value: f64) {
            let v = v.into();
            let mut inner = self.0.borrow_mut();
            if let [(idx, coeff)] = v.terms() {
                if (coeff - 1.0).abs() < EPS {
                    inner.edits.insert(*idx, value - v.constant);
                    inner.dirty = true;
                }
            }
        }

        pub fn is_feasible(&self) -> bool {
            self.resolve();
            self.0.borrow().infeasible.is_none()
        }

        /// Labels of the required constraints the simplex identified as
        /// mutually contradictory (empty when feasible).
        pub fn conflict(&self) -> Vec<String> {
            self.resolve();
            self.0.borrow().infeasible.clone().unwrap_or_default()
        }

        fn resolve(&self) {
            let mut inner = self.0.borrow_mut();
            if !inner.dirty {
                return;
            }
            solve(&mut inner);
            inner.dirty = false;
        }

        fn push_row(&self, row: RowSpec) -> usize {
            let mut inner = self.0.borrow_mut();
            inner.rows.push(row);
            inner.dirty = true;
            inner.rows.len() - 1
        }
    }

    /// D-LAYOUT1: a registered, prioritizable constraint handle. Chains
    /// `.required()`/`.strong()`/`.medium()`/`.weak()` to change its priority
    /// after the fact (mutates the shared handle's stored row in place).
    #[derive(Clone)]
    pub struct Constraint {
        handle: Rc<RefCell<Inner>>,
        idx: usize,
    }

    impl Constraint {
        fn set_strength(self, s: Strength) -> Constraint {
            {
                let mut inner = self.handle.borrow_mut();
                if let Some(row) = inner.rows.get_mut(self.idx) {
                    row.strength = s;
                }
                inner.dirty = true;
            }
            self
        }

        pub fn required(self) -> Constraint {
            self.set_strength(Strength::Required)
        }

        pub fn strong(self) -> Constraint {
            self.set_strength(Strength::Strong)
        }

        pub fn medium(self) -> Constraint {
            self.set_strength(Strength::Medium)
        }

        pub fn weak(self) -> Constraint {
            self.set_strength(Strength::Weak)
        }
    }

    fn make_constraint(lhs: LinExpr, rhs: LinExpr, op: RelOp) -> Constraint {
        // Either side's `handle` id resolves the SAME registered `Inner` —
        // both ultimately came from the same `layout {}` block's handle.
        let handle_id = lhs
            .handle
            .or(rhs.handle)
            .unwrap_or_else(|| Handle::new("").1);
        let handle = handle_by_id(handle_id);
        let row = build_row(&lhs, &rhs, op, Strength::Required, String::new());
        let idx = Handle(handle.clone(), handle_id).push_row(row);
        {
            // Fill in a readable label now that we know the row's index.
            let mut inner = handle.borrow_mut();
            if let Some(r) = inner.rows.get_mut(idx) {
                let op_str = match op {
                    RelOp::Le => "<=",
                    RelOp::Ge => ">=",
                    RelOp::Eq => "==",
                };
                r.label = format!("constraint#{} ({})", idx, op_str);
            }
        }
        Constraint { handle, idx }
    }

    /// D-LAYOUT1 / D-LAYOUT-GATES1 (GATE 1): `lhs >= rhs` between layout values.
    pub fn ge(lhs: impl Into<LinExpr>, rhs: impl Into<LinExpr>) -> Constraint {
        make_constraint(lhs.into(), rhs.into(), RelOp::Ge)
    }

    /// D-LAYOUT1 / D-LAYOUT-GATES1 (GATE 1): `lhs <= rhs` between layout values.
    pub fn le(lhs: impl Into<LinExpr>, rhs: impl Into<LinExpr>) -> Constraint {
        make_constraint(lhs.into(), rhs.into(), RelOp::Le)
    }

    /// D-LAYOUT1 / D-LAYOUT-GATES1 (GATE 1): `lhs == rhs` between layout values.
    /// Named `eq_` (not `eq`) to stay clear of `PartialEq::eq`.
    pub fn eq_(lhs: impl Into<LinExpr>, rhs: impl Into<LinExpr>) -> Constraint {
        make_constraint(lhs.into(), rhs.into(), RelOp::Eq)
    }
}
