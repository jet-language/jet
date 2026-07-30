use crate::AST::Stmt;
use crate::Sema::Captures::stmt_refs_name;
use crate::Sema::Checker;
impl<'a> Checker<'a> {
        // --- statements -----------------------------------------------------
    
        pub(crate) fn check_block(&mut self, stmts: &mut [Stmt], new_scope: bool) {
            if new_scope {
                self.push_scope();
            }
            // E0209 liveness gate (was D-L0201): before checking each statement,
            // record the tail of the current block (statements that follow it).
            // The helper `is_name_live_after` reads this to word the E0209 fix
            // menu (move vs. copy/reorder). We push the previous frame onto the
            // liveness_frames stack so `is_name_live_after` can walk enclosing
            // scopes; on exit we pop and restore.
            let saved_ptr = self.stmt_tail_ptr;
            let saved_len = self.stmt_tail_len;
            // Push the caller's frame as an enclosing scope (non-null only).
            let pushed_frame = !saved_ptr.is_null();
            if pushed_frame {
                self.liveness_frames.push((saved_ptr, saved_len));
            }
            for i in 0..stmts.len() {
                // tail = stmts[i+1..], i.e. the statements after index i.
                let tail = &stmts[i + 1..];
                self.stmt_tail_ptr = tail.as_ptr();
                self.stmt_tail_len = tail.len();
                self.views_used_in_stmt.clear();
                self.check_stmt(&mut stmts[i]);
            }
            if pushed_frame {
                self.liveness_frames.pop();
            }
            self.stmt_tail_ptr = saved_ptr;
            self.stmt_tail_len = saved_len;
            if new_scope {
                self.pop_scope();
            }
        }
    
        /// E0209 liveness gate (was D-L0201): returns `true` when `name` is
        /// referenced in any statement that follows the current statement in the
        /// innermost block. E0209 fires either way now (no clone is ever silent),
        /// but this decides its fix menu: live-after means `^` would break that
        /// later use, so the menu offers copy/reorder; dead-after means `^` is
        /// safe (this is the value's last use).
        ///
        /// Checks the current block's tail AND all enclosing block tails pushed
        /// by `check_block`, so a clone inside a nested `if` body is not flagged
        /// when the value is used again in the enclosing block after the `if`.
        pub(crate) fn is_name_live_after(&self, name: &str) -> bool {
            // Check the innermost block's tail first.
            if !self.stmt_tail_ptr.is_null() && self.stmt_tail_len > 0 {
                // SAFETY: stmt_tail_ptr + stmt_tail_len describe a valid slice that was
                // set from `&stmts[i+1..]` just before the current check_stmt call.
                // The slice's data lives in the Program AST, which is `&mut Program`
                // at the call site and outlives the Checker.  We only read (no writes)
                // and only during `check_stmt`, so no aliasing issues.
                let tail =
                    unsafe { std::slice::from_raw_parts(self.stmt_tail_ptr, self.stmt_tail_len) };
                if tail.iter().any(|s| stmt_refs_name(s, name)) {
                    return true;
                }
            }
            // Walk enclosing frames (innermost pushed last) — if the name appears
            // in any enclosing block after the point this nested block was entered,
            // the clone is necessary.
            for &(ptr, len) in self.liveness_frames.iter().rev() {
                if !ptr.is_null() && len > 0 {
                    // SAFETY: same as above — each frame was set from a block slice
                    // in the Program AST that outlives the Checker.
                    let frame = unsafe { std::slice::from_raw_parts(ptr, len) };
                    if frame.iter().any(|s| stmt_refs_name(s, name)) {
                        return true;
                    }
                }
            }
            false
        }

        pub(crate) fn lexical_tail_len(&self) -> usize {
            self.stmt_tail_len
                + self
                    .liveness_frames
                    .iter()
                    .map(|(_, len)| *len)
                    .sum::<usize>()
        }
    
}
