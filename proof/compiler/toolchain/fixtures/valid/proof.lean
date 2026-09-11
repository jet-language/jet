def jetCompilerProofClaimId : String := "compiler-proof.identity-direct"
def jetCompilerProofProposition : String := "identity(value) returns value"
def jetCompilerProofModelPath : String := "proof/compiler/toolchain/fixtures/valid/model.lean"
def jetCompilerProofModelDigest : String := "sha256-9e3363e991dada0bf7ab4fb23cdbc2b26db1f56a8dc648fd997f8fd4691e1cef"
def jetCompilerProofImplementationPath : String := "proof/compiler/toolchain/fixtures/valid/implementation.rs"
def jetCompilerProofImplementationDigest : String := "sha256-c2a494eb9bba5c3eec6cd1c7d2545ebd5dce35d21572f0f29d80ba81c9725288"
def jetCompilerProofFormalGoal : String := "∀ (value : Nat), jetImplementation value = jetModel value"
def jetCompilerProofMethod : String := "direct_verification"


theorem jet_binding_identity (value : Nat) : jetImplementation value = jetModel value := by
  rfl
