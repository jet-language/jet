// D-TIER-ONEIR1=A: the one target-neutral iterator cursor kernel.
//
// Collections.rs supplies source-kind and JetLoopSource policy. This fragment
// owns only cursor state and stepping so native MIR adapters can retain typed
// iterators without changing the non-Send AOT/comptime carriers.

type JetLoopAny = Box<dyn std::any::Any>;

pub(crate) trait JetLoopItem {
    type Value;
    type Error: Clone;

    fn into_value(self) -> Result<Self::Value, Self::Error>;
    fn error(&self) -> Option<&Self::Error>;
    fn exhausted_error() -> Self::Error;
}

impl JetLoopItem for JetLoopAny {
    type Value = JetLoopAny;
    type Error = String;

    fn into_value(self) -> Result<Self::Value, Self::Error> {
        Ok(self)
    }

    fn error(&self) -> Option<&Self::Error> {
        None
    }

    fn exhausted_error() -> Self::Error {
        "iterator loop value requested after exhaustion".to_string()
    }
}

impl<T, E> JetLoopItem for Result<T, E>
where
    E: Clone + From<&'static str>,
{
    type Value = T;
    type Error = E;

    fn into_value(self) -> Result<Self::Value, Self::Error> {
        self
    }

    fn error(&self) -> Option<&Self::Error> {
        self.as_ref().err()
    }

    fn exhausted_error() -> Self::Error {
        E::from("iterator loop value requested after exhaustion")
    }
}

pub(crate) struct JetLoopIterCursor<
    T = JetLoopAny,
    I = Box<dyn Iterator<Item = T>>,
> {
    iter: I,
    current: Option<T>,
    step: usize,
    exhausted: bool,
}

fn jet_loop_iter_step(step_value: i64, has_step: bool) -> Result<usize, &'static str> {
    let step = if has_step { step_value } else { 1 };
    if step <= 0 {
        return Err("iterator loop stride must be positive");
    }
    usize::try_from(step).map_err(|_| "iterator loop stride is too large")
}

pub(crate) fn jet_loop_iter_init_typed<T, I>(
    mut iter: I,
    step_value: i64,
    has_step: bool,
) -> Result<JetLoopIterCursor<T, I>, &'static str>
where
    T: JetLoopItem,
    I: Iterator<Item = T>,
{
    let step = jet_loop_iter_step(step_value, has_step)?;
    let current = iter.next();
    let exhausted = current.is_none();
    Ok(JetLoopIterCursor {
        iter,
        current,
        step,
        exhausted,
    })
}

pub(crate) fn jet_loop_iter_typed_has_next<T, I>(
    cursor: &JetLoopIterCursor<T, I>,
) -> Result<bool, T::Error>
where
    T: JetLoopItem,
    I: Iterator<Item = T>,
{
    match cursor.current.as_ref() {
        Some(item) => match item.error() {
            Some(error) => Err(error.clone()),
            None => Ok(true),
        },
        None => Ok(false),
    }
}

pub(crate) fn jet_loop_iter_typed_value<T, I>(
    cursor: &mut JetLoopIterCursor<T, I>,
) -> Result<<T as JetLoopItem>::Value, T::Error>
where
    T: JetLoopItem,
    I: Iterator<Item = T>,
{
    cursor
        .current
        .take()
        .ok_or_else(T::exhausted_error)?
        .into_value()
}

// A native borrowed loop binding lends the buffered item until advance. Keep
// its failure/exhaustion meaning in the same cursor kernel as the owned pull.
pub(crate) fn jet_loop_iter_typed_value_ref<T, E, I>(
    cursor: &JetLoopIterCursor<Result<T, E>, I>,
) -> Result<&T, E>
where
    E: Clone + From<&'static str>,
    I: Iterator<Item = Result<T, E>>,
{
    cursor.current.as_ref()
        .ok_or_else(<Result<T, E> as JetLoopItem>::exhausted_error)?
        .as_ref().map_err(Clone::clone)
}

pub(crate) fn jet_loop_iter_typed_advance<T, I>(
    cursor: &mut JetLoopIterCursor<T, I>,
) -> Result<(), T::Error>
where
    T: JetLoopItem,
    I: Iterator<Item = T>,
{
    if cursor.exhausted {
        return Ok(());
    }
    for _ in 1..cursor.step {
        let Some(item) = cursor.iter.next() else {
            cursor.current = None;
            cursor.exhausted = true;
            return Ok(());
        };
        if item.error().is_some() {
            cursor.current = None;
            cursor.exhausted = true;
            let Err(error) = item.into_value() else {
                unreachable!("iterator item reported an error but yielded a value")
            };
            return Err(error);
        }
    }
    cursor.current = cursor.iter.next();
    cursor.exhausted = cursor.current.is_none();
    Ok(())
}

/// Lazy UTF-8 character source shared by the AOT and native MIR adapters.
pub(crate) struct JetStringChars {
    bytes: Vec<u8>,
    offset: usize,
}

impl JetStringChars {
    pub(crate) fn new(value: String) -> Self {
        Self {
            bytes: value.into_bytes(),
            offset: 0,
        }
    }
}

impl Iterator for JetStringChars {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        let end = self.offset.saturating_add(4).min(self.bytes.len());
        let bytes = self.bytes.get(self.offset..end)?;
        let tail = match std::str::from_utf8(bytes) {
            Ok(tail) => tail,
            Err(error) => std::str::from_utf8(&bytes[..error.valid_up_to()]).ok()?,
        };
        let value = tail.chars().next()?;
        self.offset += value.len_utf8();
        Some(value)
    }
}

fn jet_loop_iter_has_next(cursor: &JetLoopIterCursor) -> bool {
    jet_loop_iter_typed_has_next(cursor)
        .unwrap_or_else(|message| jet_panic("<core.prelude>", 0, &message))
}

fn jet_loop_iter_value<T: 'static>(cursor: &mut JetLoopIterCursor) -> T {
    let value = jet_loop_iter_typed_value(cursor)
        .unwrap_or_else(|message| jet_panic("<core.prelude>", 0, &message));
    match value.downcast::<T>() {
        Ok(value) => *value,
        Err(_) => jet_panic("<core.prelude>", 0, "iterator loop item type does not match MIR"),
    }
}

fn jet_loop_iter_advance(cursor: &mut JetLoopIterCursor) {
    jet_loop_iter_typed_advance(cursor)
        .unwrap_or_else(|message| jet_panic("<core.prelude>", 0, &message));
}
