    // ── D-DBPOLICY1=A: the one closed row-policy language ──────────────────────
    // A row policy is `db.policy(table, expression)`. The accepted table shape,
    // the length bound, and the expression table are ONE fact, compiled once
    // here into `JetRowPolicyExpr`. Every tier includes this fragment, so no
    // engine re-derives the rule (I9): AOT splices it into `mod jet_std`
    // (`Codegen/mod.rs`'s `CORELIB_KERNEL_PARTS`), the Cranelift host and the
    // ambient interpreter include it into their `mod wire`
    // (`jet-jit/src/DB.rs`, `jet-jit/src/ambient_interp.rs`), and comptime
    // includes it into its `jet_std` mirror (`Comptime/SyncJetStd.rs`).
    //
    // Callers keep the COMPILED form and render through `canonical()`. Nothing
    // downstream re-recognizes the caller's original spelling, so a scope
    // cannot execute a policy the constructor would have rejected, and no two
    // tiers can disagree about which policies exist.
    pub const JET_ROW_POLICY_MAX_TEXT: usize = 1024 * 1024;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum JetRowPolicyExpr {
        AllowAll,
        OwnerEqualsUser,
    }

    impl JetRowPolicyExpr {
        /// The one spelling of each compiled form — used for display, for the
        /// SQL transformer's re-check, and for any tier that must park the
        /// policy as text between calls.
        pub fn canonical(self) -> &'static str {
            match self {
                JetRowPolicyExpr::AllowAll => "true",
                JetRowPolicyExpr::OwnerEqualsUser => "owner == user",
            }
        }

        /// The compiled predicate consumed by the SQL transformer. The
        /// expression enum is the one policy language; this method is its
        /// single lowering to a bind-safe SQL predicate.
        pub fn sql_predicate(self) -> &'static str {
            match self {
                JetRowPolicyExpr::AllowAll => "true",
                JetRowPolicyExpr::OwnerEqualsUser => "owner = ?",
            }
        }

        pub fn requires_owner_filter(self) -> bool {
            matches!(self, JetRowPolicyExpr::OwnerEqualsUser)
        }
    }

    /// Render the compiled policy facts for an active scope. This is audit
    /// output, not a second parser: every field comes from the shared
    /// `JetRowPolicyExpr` lowering above.
    pub fn jet_db_policy_audit_line(
        table: &str,
        compiled: JetRowPolicyExpr,
        user: &str,
    ) -> String {
        format!(
            "DBPolicy(table={table}, user={user}, expr={}, predicate={})",
            compiled.canonical(),
            compiled.sql_predicate(),
        )
    }

    pub fn jet_db_policy_validate_table(table: &str) -> Result<String, String> {
        let table = table.trim();
        if table.is_empty()
            || table.len() > JET_ROW_POLICY_MAX_TEXT
            || table.chars().any(char::is_control)
        {
            return Err("row policy needs a table and expression".to_string());
        }
        let mut chars = table.chars();
        let head_ok = match chars.next() {
            Some(first) => first.is_ascii_alphabetic() || first == '_',
            None => false,
        };
        if !head_ok || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err("row policy table must be a simple identifier".to_string());
        }
        Ok(table.to_string())
    }

    /// Compile the closed policy language. `Ok` carries the normalized table
    /// name and the compiled expression; a rejection is one message, identical
    /// on every tier. Compiling an already-compiled pair is idempotent.
    pub fn jet_db_policy_compile(
        table: &str,
        expression: &str,
    ) -> Result<(String, JetRowPolicyExpr), String> {
        let expression = expression.trim();
        let table = jet_db_policy_validate_table(table)?;
        if expression.is_empty()
            || expression.len() > JET_ROW_POLICY_MAX_TEXT
            || expression.chars().any(char::is_control)
        {
            return Err("row policy needs a table and expression".to_string());
        }
        let compiled = match expression {
            "true" => JetRowPolicyExpr::AllowAll,
            "owner == user" => JetRowPolicyExpr::OwnerEqualsUser,
            other => {
                return Err(format!(
                    "unsupported row policy expression `{other}`; supported forms are `true` and `owner == user`"
                ));
            }
        };
        Ok((table, compiled))
    }
