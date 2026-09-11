def jetCompilerProofClaimId : String := "compiler-proof.identity-direct"
def jetCompilerProofProposition : String := "identity(value) returns value"
def jetCompilerProofFormalGoal : String := "jetImplementation value = jetModel value"
def jetCompilerProofModelPath : String := "proof/compiler/toolchain/fixtures/valid/model.lean"
def jetCompilerProofModelDigest : String := "sha256-438e7a89c65ac64054276ab93a05207bc81e7b27537bc819c2da7adb670670a5"
def jetCompilerProofImplementationPath : String := "proof/compiler/toolchain/fixtures/valid/implementation.rs"
def jetCompilerProofImplementationDigest : String := "sha256-c2a494eb9bba5c3eec6cd1c7d2545ebd5dce35d21572f0f29d80ba81c9725288"
def jetCompilerProofMethod : String := "direct_verification"
def jetModel (value : Nat) : Nat := value
def jetImplementation (value : Nat) : Nat := value

theorem jet_binding_identity (value : Nat) : value = value := by
  rfl
