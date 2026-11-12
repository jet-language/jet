# Types

Structs, enums, optionals, traits, patterns and units of measure.

Run a lesson from the repository root, for example `jet run Examples/Types/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Lessons

- [AnonymousUnions](AnonymousUnions.jet) — Anonymous unions are structural enum sugar.
- [AsField](AsField.jet) — `#As` on one stored field lets the struct pass where that field's type is expected, read as that field.
- [Authority](Authority/) — Authority is the one rights value that crosses process, plugin, and session boundaries.
- [CapabilityBundles](CapabilityBundles.jet) — Operation bundles on nominal `distinct` types.
- [ConstructionOrder](ConstructionOrder.jet) — struct literals and named enum payloads evaluate their field values in the order they are written, not the order the type declares its fields.
- [DistinctForwarding](DistinctForwarding.jet) — A distinct type forwards the traits it names from its base type, and each forwarded member behaves exactly as the base type's: `#Comparable` orders a…
- [DistinctTypes](DistinctTypes.jet) — Distinct types.
- [DotConstruction](DotConstruction.jet)
- [EnumBacking](EnumBacking.jet) — A payload-free enum names its backing integer with `#Layout(U8)`.
- [EnumDerivedOrder](EnumDerivedOrder.jet) — Derived enum equality and ordering: different variants order by declaration position; the same variant compares its payload fields in order, recursing…
- [EnumDot](EnumDot.jet) — leading-dot enum variant `.Variant`.
- [EnumSpecified](EnumSpecified.jet) — `#Specified` makes every case of a numbered enum state its number, so a case added or moved later renumbers no other: these are on-disk values.
- [Enums](Enums.jet) — Enums and exhaustive when.
- [FloatToF32](FloatToF32.jet) — `F32{x}` narrows a Float to the nearest F32 and stops when `x` is out of F32's range; `F32.from_float(x)` reports that case as an Err instead.
- [InBodyMarkers](InBodyMarkers.jet) — A declaration's markers may be written as lines inside its body: `#X: value` is the same marker as `#X(value)` above the declaration.
- [InferredNew](InferredNew.jet) — Fresh hidden-state construction may omit its receiver when the surrounding expected type selects exactly one type.
- [ListElementInference](ListElementInference.jet) — List elements infer one type from a named head or annotation.
- [MultiHeadFunctions](MultiHeadFunctions.jet) — Ordered enum-shaped heads fold into one table.
- [NestedEnumGroups](NestedEnumGroups.jet)
- [NoAnyAlternatives](NoAnyAlternatives.jet) — Jet has no top type like Any.
- [Patchable](Patchable.jet)
- [RangeTypes](RangeTypes.jet)
- [RecursiveEnum](RecursiveEnum.jet)
- [RecursiveNamedVariant](RecursiveNamedVariant.jet) — Recursive named-payload variants: `IntExpr.Add{…}` boxes each recursive field invisibly, and derived equality and ordering recurse through them.
- [RecursiveRecord](RecursiveRecord.jet) — Value copies form finite snapshots.
- [Refinements](Refinements.jet) — One interval fact feeds the fixed-list index prover, so `faces[roll.raw() - 1]` needs no runtime bounds check.
- [RenderableVarargs](RenderableVarargs.jet) — `parts: ...Renderable` — a trait-bounded heterogeneous variadic.
- [SelfReturn](SelfReturn.jet) — `Self` in a method signature names the impl's type: a static `-> Self` method returns that type, so methods chain on its result.
- [Structs](Structs.jet)
- [TypeAlias](TypeAlias.jet) — Transparent generic type aliases shorten fallible signatures.
- [TypedLiteralForms](TypedLiteralForms.jet) — Universal typed-literal Type{ body } form.
- [TypeMarksSuffix](TypeMarksSuffix.jet) — `?` and `!` follow the type they mark.
- [TypeState](TypeState.jet) — Typestate: order-of-events safety, checked at compile time.

## Generics

- [ConstPacket](Generics/ConstPacket.jet) — A number parameter, `<N: Int>`, makes one instance per number: a `Packet<4>` and a `Packet<2>` are two types with their own lengths, and a…

## Optionals

- [Option](Optionals/Option.jet) — Option with pattern tests and flow narrowing.
- [OptionalLifting](Optionals/OptionalLifting.jet) — A plain value fills a slot whose type is already `T?`.
- [OptionalMethodChain](Optionals/OptionalMethodChain.jet)
- [OptionalResultVariants](Optionals/OptionalResultVariants.jet) — Optional and Result use one PascalCase variant law.
- [OptionalTypedHead](Optionals/OptionalTypedHead.jet) — `T?{ body }` is the optional typed-literal head.
- [OptionCombinators](Optionals/OptionCombinators.jet) — `.zip`/`Option.lift2` — Option composition without a general "hole"/absent-propagating value type.
- [OptionExitGuard](Optionals/OptionExitGuard.jet) — After an `x == None` guard that always leaves, the rest of the block reads `x` as its payload.

## Patterns

- [ConstantAlternates](Patterns/ConstantAlternates.jet) — An ALL_CAPS name in an arm head is a constant, never a bare enum variant, so `ARCH_X64 | ARCH_ARM64` lists two value alternates and needs no leading `.`;…
- [MapPatterns](Patterns/MapPatterns.jet) — Exact keys by default, ..
- [NamedVariantPatterns](Patterns/NamedVariantPatterns.jet) — A case declared with named fields is matched by the names it is built with, at any depth: `.FailedToFetch{url, cause: .BadUrl(why)}`.
- [NestedVariantPatterns](Patterns/NestedVariantPatterns.jet)
- [RecordListPatterns](Patterns/RecordListPatterns.jet) — The pattern has the shape of the value it reads.
- [RefutableTestBind](Patterns/RefutableTestBind.jet) — Test the subject first; the route leaves on a miss.
- [StructDestructure](Patterns/StructDestructure.jet) — The dot-construction shape runs backwards to destructure.
- [TupleFieldPatterns](Patterns/TupleFieldPatterns.jet) — Text and binary patterns synthesize structural capture tuples.
- [UnionMemberArms](Patterns/UnionMemberArms.jet) — A union arm may name a member type without binding its value as a bare arm: `.Int -> …` tests the member exactly as `.Int(_)` does.
- [VariantTestValue](Patterns/VariantTestValue.jet) — A variant test whose payload binds nothing is a Bool value in expression position, including a field-path subject and an or-pattern.

## Traits

- [AssociatedTypes](Traits/AssociatedTypes.jet) — associated types on a trait.
- [BuiltinTargets](Traits/BuiltinTargets.jet) — `impl Int.Named`: a trait the module declares, implemented for built-in types.
- [DisplayDebug](Traits/DisplayDebug.jet) — Display vs Debug interpolation + #Redact
- [ExternalMethods](Traits/ExternalMethods.jet)
- [TagsAndTraits](Traits/TagsAndTraits.jet) — Methods → trait; no methods → tag.
- [TraitConstants](Traits/TraitConstants.jet) — Trait constants: a trait declares `name: Type` (with a default as `name: Type{value}`), each implementation gives `name: value`, and code reads `Type.name`…
- [Traits](Traits/Traits.jet)
- [TraitValueSlots](Traits/TraitValueSlots.jet)
- [UserCheckedTextShape](Traits/UserCheckedTextShape.jet) — A user trait with the shape of the Prelude's `CheckedText`: one associated error type, a pure checker that fails with it, and a pure generic encoder.
- [UserDefined](Traits/UserDefined.jet) — Existing symbols dispatch through ordinary trait hooks.
- [UserDisplayDebug](Traits/UserDisplayDebug.jet)

## Units

- [AffineUnitTypes](Units/AffineUnitTypes.jet) — An affine family mints separate zero-overhead Point and Delta types, while exact scale/offset facts remain part of the package API identity.
- [DimensionalQuantities](Units/DimensionalQuantities.jet) — Standard units come from ordinary Prelude/Units.jet declarations.
- [Measurement](Units/Measurement.jet)
- [UnitFamily](Units/UnitFamily.jet) — Unit families.
- [UnitLiterals](Units/UnitLiterals.jet) — A numeric literal may carry a unit suffix resolving to a Time literals are members of the canonical Prelude Time family.
