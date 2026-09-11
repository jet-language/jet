def jetModel (value : Nat) : Nat := value
def jetImplementation (value : Nat) : Nat := value

theorem jetModel_identity (value : Nat) : jetModel value = value := by
  rfl
