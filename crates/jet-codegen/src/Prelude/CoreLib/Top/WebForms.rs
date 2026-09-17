// D-DX-SUITE1=C: first-party Forms state on the shared reactive primitive.
//
// Field descriptors are supplied by the type-driven compiler projection. This
// runtime owns the common validation, async validation, action submission, and
// no-script HTML contract; renderers consume the same headless state.

use std::collections::BTreeMap as JetWebFormMap;
use std::sync::atomic::{AtomicU64, Ordering as JetWebFormOrdering};
use std::sync::{Arc as JetWebFormArc, Mutex as JetWebFormMutex};

const JET_WEB_FORM_MAX_FIELDS: usize = 256;
const JET_WEB_FORM_MAX_NAME: usize = 256;
const JET_WEB_FORM_MAX_VALUE: usize = 1024 * 1024;
const JET_WEB_FORM_MAX_BODY: usize = 4 * 1024 * 1024;
fn jet_web_form_json(value: &str) -> String {
    format!("{value:?}")
}

fn jet_web_form_event_entity(action: &str) -> String {
    if !action.trim().is_empty()
        && action.len() <= JET_WEB_FORM_MAX_NAME
        && action.chars().all(|character| !character.is_control())
    {
        action.to_string()
    } else {
        "form".to_string()
    }
}

fn jet_web_form_publish_state(state: &JetWebFormState, cause: &str) {
    if !jet_web_runtime_devtools_enabled() {
        return;
    }
    let invalid = state.fields.values().filter(|field| !field.errors.is_empty()).count();
    let validating = state.fields.values().filter(|field| field.validating).count();
    let touched = state.fields.values().filter(|field| field.touched).count();
    let fields = format!(
        "{{\"status\":{},\"field_count\":{},\"invalid_count\":{},\"validating_count\":{},\"touched_count\":{},\"submissions\":{},\"error_bytes\":{},\"result_bytes\":{},\"cause\":{}}}",
        jet_web_form_json(state.status.name()),
        state.fields.len(),
        invalid,
        validating,
        touched,
        state.submission_count,
        state.error.len(),
        state.result.len(),
        jet_web_form_json(cause),
    );
    if let Ok(event) = jet_foundation::Devtools::JetDevtoolsEvent::from_parts(
        jet_web_form_now_ms(),
        "core.web.forms",
        "Form",
        jet_web_form_event_entity(&state.action),
        fields,
    ) {
        jet_devtools_publish_event(event);
    }
}
const JET_WEB_FORM_MAX_ERROR: usize = 4096;
fn jet_web_form_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn jet_web_form_publish_lifecycle(
    type_name: &str,
    lifecycle: &JetWebFormLifecycle,
    state: &JetWebFormState,
) {
    if !jet_web_runtime_devtools_enabled() {
        return;
    }
    let entity = if jet_web_form_valid_type_name(type_name) {
        type_name.to_string()
    } else {
        "typed-form".to_string()
    };
    let fields = format!(
        "{{\"lifecycle\":{},\"generation\":{},\"form_status\":{},\"error_bytes\":{},\"result_bytes\":{}}}",
        jet_web_form_json(lifecycle.status.name()),
        lifecycle.generation,
        jet_web_form_json(state.status.name()),
        lifecycle.error.len(),
        lifecycle.result.len(),
    );
    if let Ok(event) = jet_foundation::Devtools::JetDevtoolsEvent::from_parts(
        jet_web_form_now_ms(),
        "core.web.forms",
        "Form",
        entity,
        fields,
    ) {
        jet_devtools_publish_event(event);
    }
}

#[derive(Clone)]
struct JetWebFormPanelFact {
    id: u64,
    type_name: String,
    action: String,
    fields: Vec<JetWebFormFieldSpec>,
    state: jet_std::JetSignal<JetWebFormState>,
    lifecycle: jet_std::JetSignal<JetWebFormLifecycle>,
}

static JET_WEB_FORM_PANEL_FACTS: std::sync::LazyLock<JetWebFormMutex<Vec<JetWebFormPanelFact>>> =
    std::sync::LazyLock::new(|| JetWebFormMutex::new(Vec::new()));
static JET_WEB_FORM_PANEL_NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn jet_web_form_register_panel_fact(form: &JetWebFormTyped) {
    if !jet_web_runtime_devtools_enabled() {
        return;
    }
    let fact = JetWebFormPanelFact {
        id: JET_WEB_FORM_PANEL_NEXT_ID.fetch_add(1, JetWebFormOrdering::Relaxed),
        type_name: form.input.type_name.clone(),
        action: form.form.state().action,
        fields: form.input.fields(),
        state: form.form.state_signal(),
        lifecycle: form.lifecycle.clone(),
    };
    if let Ok(mut facts) = JET_WEB_FORM_PANEL_FACTS.lock() {
        facts.push(fact);
    }
}

fn jet_web_form_panel_values(state: &JetWebFormState) -> String {
    state
        .fields
        .iter()
        .map(|(name, field)| {
            format!(
                "{}={}",
                jet_web_form_escape(name),
                jet_web_form_escape(&field.value)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn jet_web_form_panel_names(
    state: &JetWebFormState,
    select: impl Fn(&JetWebFormFieldState) -> bool,
) -> String {
    state
        .fields
        .iter()
        .filter(|(_, field)| select(field))
        .map(|(name, _)| jet_web_form_escape(name))
        .collect::<Vec<_>>()
        .join(",")
}

/// Development-only form state. Values are intentionally visible here because
/// this panel is opt-in dev output; release HTML never calls this function.
pub(crate) fn jet_web_form_dev_panel() -> String {
    if !jet_web_runtime_devtools_enabled() {
        return String::new();
    }
    let Ok(facts) = JET_WEB_FORM_PANEL_FACTS.lock() else {
        return String::new();
    };
    let mut rows = Vec::new();
    for fact in facts.iter() {
        let state = fact.state.get();
        let lifecycle = fact.lifecycle.get();
        let field_errors = state
            .fields
            .iter()
            .filter(|(_, field)| !field.errors.is_empty())
            .map(|(name, field)| {
                format!(
                    "{}={}",
                    jet_web_form_escape(name),
                    jet_web_form_escape(&field.errors.join("; "))
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let validation_phase = if state.fields.values().any(|field| field.validating) {
            "validating"
        } else {
            state.status.name()
        };
        let facts_text = format!(
            "values=[{}],touched=[{}],validation_phase={},lifecycle={},submissions={},field_errors=[{}],form_error={}",
            jet_web_form_panel_values(&state),
            jet_web_form_panel_names(&state, |field| field.touched),
            validation_phase,
            lifecycle.status.name(),
            state.submission_count,
            field_errors,
            jet_web_form_escape(&state.error),
        );
        rows.push(format!(
            "<li data-jet-form-id=\"{}\" data-jet-form-type=\"{}\" data-jet-form-action=\"{}\"><code>{}</code><pre>{}</pre></li>",
            fact.id,
            jet_web_form_escape(&fact.type_name),
            jet_web_form_escape(&fact.action),
            jet_web_form_escape(&fact.type_name),
            jet_web_form_escape(&facts_text),
        ));
    }
    if rows.is_empty() {
        return String::new();
    }
    format!(
        "<aside id=\"jet-dev-forms\" data-jet-devtools=\"forms\"><h2>Forms</h2><ul>{}</ul></aside>",
        rows.join("")
    )
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebFormValueType {
    String,
    Int,
    Bool,
    Float,
}

impl JetWebFormValueType {
    fn name(self) -> &'static str {
        match self {
            Self::String => "String",
            Self::Int => "Int",
            Self::Bool => "Bool",
            Self::Float => "Float",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebFormStatus {
    Idle,
    Dirty,
    Validating,
    Invalid,
    Submitting,
    Submitted,
    Error,
}

impl JetWebFormStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Dirty => "dirty",
            Self::Validating => "validating",
            Self::Invalid => "invalid",
            Self::Submitting => "submitting",
            Self::Submitted => "submitted",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebFormFieldState {
    pub name: String,
    pub value_type: JetWebFormValueType,
    pub required: bool,
    pub value: String,
    pub touched: bool,
    pub validating: bool,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct JetWebFormState {
    pub status: JetWebFormStatus,
    pub fields: JetWebFormMap<String, JetWebFormFieldState>,
    pub action: String,
    pub method: String,
    pub result: String,
    pub error: String,
    pub submission_count: u64,
}

impl JetWebFormState {
    fn new(action: String) -> Self {
        Self {
            status: JetWebFormStatus::Idle,
            fields: JetWebFormMap::new(),
            action,
            method: "post".to_string(),
            result: String::new(),
            error: String::new(),
            submission_count: 0,
        }
    }
}

type JetWebFormValidator = JetWebFormArc<dyn Fn(String) -> Result<(), String> + Send + Sync>;
type JetWebFormAction = JetWebFormArc<dyn Fn(String) -> Result<String, String> + Send + Sync>;

#[derive(Clone)]
struct JetWebFormRules {
    validators: JetWebFormMap<String, Vec<JetWebFormValidator>>,
    async_validators: JetWebFormMap<String, Vec<JetWebFormValidator>>,
    action: Option<JetWebFormAction>,
}

#[derive(Clone)]
pub struct JetWebForm {
    state: jet_std::JetSignal<JetWebFormState>,
    rules: JetWebFormArc<JetWebFormMutex<JetWebFormRules>>,
    validation_generation: JetWebFormArc<AtomicU64>,
}

pub struct JetWebFormValidation {
    join: Option<jet_std::JetTask<Result<(), String>>>,
}

impl JetWebFormValidation {
    pub fn wait(mut self) -> Result<(), String> {
        let Some(join) = self.join.take() else {
            return Err("form validation worker is unavailable".to_string());
        };
        match join.join() {
            Ok(result) => result,
            Err(error) => Err(format!("form validation task failed: {error:?}")),
        }
    }
    pub fn cancel(&self) {
        if let Some(join) = &self.join {
            join.cancel();
        }
    }
}

impl JetWebForm {
    pub fn new(action: String) -> Self {
        Self {
            state: jet_std::JetSignal::new(JetWebFormState::new(action)),
            rules: JetWebFormArc::new(JetWebFormMutex::new(JetWebFormRules {
                validators: JetWebFormMap::new(),
                async_validators: JetWebFormMap::new(),
                action: None,
            })),
            validation_generation: JetWebFormArc::new(AtomicU64::new(0)),
        }
    }

    pub fn field(
        &self,
        name: String,
        value_type: JetWebFormValueType,
        required: bool,
    ) -> Result<Self, String> {
        if !jet_web_form_valid_name(&name) {
            return Err(format!("invalid form field name `{name}`"));
        }
        if name.len() > JET_WEB_FORM_MAX_NAME {
            return Err(format!("form field name exceeds {} bytes", JET_WEB_FORM_MAX_NAME));
        }
        let mut state = self.state.get();
        if state.fields.len() >= JET_WEB_FORM_MAX_FIELDS && !state.fields.contains_key(&name) {
            return Err("form has too many fields".to_string());
        }
        if state.fields.contains_key(&name) {
            return Err(format!("duplicate form field `{name}`"));
        }
        state.fields.insert(
            name.clone(),
            JetWebFormFieldState {
                name,
                value_type,
                required,
                value: String::new(),
                touched: false,
                validating: false,
                errors: Vec::new(),
            },
        );
        self.set_state(state, "field-added");
        self.validation_generation.fetch_add(1, JetWebFormOrdering::AcqRel);
        Ok(self.clone())
    }

    pub fn set(&self, name: String, value: String) -> Result<(), String> {
        if value.len() > JET_WEB_FORM_MAX_VALUE {
            return Err(format!("form field `{name}` exceeds {} bytes", JET_WEB_FORM_MAX_VALUE));
        }
        let mut state = self.state.get();
        let field = state
            .fields
            .get_mut(&name)
            .ok_or_else(|| format!("unknown form field `{name}`"))?;
        field.value = value;
        field.errors.clear();
        field.validating = false;
        state.status = JetWebFormStatus::Dirty;
        state.error.clear();
        self.validation_generation.fetch_add(1, JetWebFormOrdering::AcqRel);
        self.set_state(state, "field-set");
        Ok(())
    }

    pub fn blur(&self, name: String) -> Result<(), String> {
        self.blur_with_async(name, true)
    }

    fn blur_with_async(&self, name: String, run_async: bool) -> Result<(), String> {
        let mut state = self.state.get();
        let field = state
            .fields
            .get_mut(&name)
            .ok_or_else(|| format!("unknown form field `{name}`"))?;
        field.touched = true;
        field.errors.clear();
        let value = field.value.clone();
        let value_type = field.value_type;
        let required = field.required;
        self.validation_generation.fetch_add(1, JetWebFormOrdering::AcqRel);
        state.status = JetWebFormStatus::Dirty;
        self.set_state(state, "blur");
        self.validate_one(name, value, value_type, required, run_async)
    }

    pub fn set_validator<F>(&self, name: String, validator: F) -> Result<(), String>
    where
        F: Fn(String) -> Result<(), String> + Send + Sync + 'static,
    {
        if !self.state.get().fields.contains_key(&name) {
            return Err(format!("unknown form field `{name}`"));
        }
        let mut rules = self.rules.lock().map_err(|_| "form rules are unavailable".to_string())?;
        rules
            .validators
            .entry(name)
            .or_default()
            .push(JetWebFormArc::new(validator));
        Ok(())
    }

    pub fn set_async_validator<F>(&self, name: String, validator: F) -> Result<(), String>
    where
        F: Fn(String) -> Result<(), String> + Send + Sync + 'static,
    {
        if !self.state.get().fields.contains_key(&name) {
            return Err(format!("unknown form field `{name}`"));
        }
        let mut rules = self.rules.lock().map_err(|_| "form rules are unavailable".to_string())?;
        rules
            .async_validators
            .entry(name)
            .or_default()
            .push(JetWebFormArc::new(validator));
        Ok(())
    }

    pub fn set_action<F>(&self, action: F)
    where
        F: Fn(String) -> Result<String, String> + Send + Sync + 'static,
    {
        if let Ok(mut rules) = self.rules.lock() {
            rules.action = Some(JetWebFormArc::new(action));
        }
    }

    pub fn state(&self) -> JetWebFormState {
        self.state.get()
    }

    pub fn state_signal(&self) -> jet_std::JetSignal<JetWebFormState> {
        self.state.clone()
    }
    fn set_state(&self, state: JetWebFormState, cause: &str) {
        self.state.set(state.clone());
        jet_web_form_publish_state(&state, cause);
    }


    pub fn validate(&self) -> Result<(), String> {
        let generation = self.validation_generation.fetch_add(1, JetWebFormOrdering::AcqRel) + 1;
        let snapshot = self.state.get();
        let rules = self
            .rules
            .lock()
            .map_err(|_| "form rules are unavailable".to_string())?
            .clone();
        let mut errors = JetWebFormMap::new();
        for field in snapshot.fields.values() {
            let field_errors =
                jet_web_form_validate_field(field, rules.validators.get(&field.name));
            if !field_errors.is_empty() {
                errors.insert(field.name.clone(), field_errors);
            }
        }
        self.apply_validation_if_current(errors, generation)
    }

    pub fn validate_async(&self) -> JetWebFormValidation {
        let generation = self.validation_generation.fetch_add(1, JetWebFormOrdering::AcqRel) + 1;
        let snapshot = self.state.get();
        let rules = match self.rules.lock() {
            Ok(rules) => rules.clone(),
            Err(_) => {
                let join = jet_std::JetTask::spawn(|| {
                    Err("form rules are unavailable".to_string())
                });
                return JetWebFormValidation { join: Some(join) };
            }
        };
        let mut sync_errors = JetWebFormMap::new();
        let mut has_async = false;
        let mut pending = snapshot.clone();
        for field in pending.fields.values_mut() {
            let field_errors =
                jet_web_form_validate_field(field, rules.validators.get(&field.name));
            if !field_errors.is_empty() {
                sync_errors.insert(field.name.clone(), field_errors.clone());
            }
            if rules
                .async_validators
                .get(&field.name)
                .is_some_and(|validators| !validators.is_empty())
            {
                has_async = true;
                field.validating = true;
            } else {
                field.validating = false;
            }
            field.touched = true;
            field.errors = field_errors;
        }
        pending.status = if has_async {
            JetWebFormStatus::Validating
        } else if sync_errors.is_empty() {
            JetWebFormStatus::Dirty
        } else {
            JetWebFormStatus::Invalid
        };
        pending.error = if sync_errors.is_empty() {
            String::new()
        } else {
            sync_errors
                .values()
                .flat_map(|items| items.iter())
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")
        };
        self.set_state(pending, "validation-start");

        if !has_async {
            let validation_result = if sync_errors.is_empty() {
                Ok(())
            } else {
                Err(sync_errors
                    .values()
                    .flat_map(|items| items.iter())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; "))
            };
            let join = jet_std::JetTask::spawn(move || validation_result);
            return JetWebFormValidation { join: Some(join) };
        }
        let form = self.clone();
        let join = jet_std::JetTask::spawn(move || {
            let mut errors = sync_errors;
            for field in snapshot.fields.values() {
                let mut field_errors = errors.remove(&field.name).unwrap_or_default();
                if let Some(validators) = rules.async_validators.get(&field.name) {
                    for validator in validators {
                        if let Err(error) = validator(field.value.clone()) {
                            field_errors.push(jet_web_form_bound_error(error));
                        }
                    }
                }
                if !field_errors.is_empty() {
                    errors.insert(field.name.clone(), field_errors);
                }
            }
            if !form.validation_current(generation, &snapshot) {
                return Ok(());
            }
            form.apply_validation(errors)
        });
        JetWebFormValidation { join: Some(join) }
    }


    pub fn submit(&self) -> Result<String, String> {
        self.validate_async().wait()?;
        let body = self.encode_submission()?;
        self.submit_body_after_validation(body)
    }

    /// Dispatch an already encoded body through the same checked action path
    /// used by `submit`.  Typed forms use this entry point after their input
    /// contract has selected renamed wire keys.
    pub fn submit_body(&self, body: String) -> Result<String, String> {
        if body.len() > JET_WEB_FORM_MAX_BODY {
            return Err(format!(
                "encoded form body exceeds {} bytes",
                JET_WEB_FORM_MAX_BODY
            ));
        }
        self.validate_async().wait()?;
        self.submit_body_after_validation(body)
    }

    fn submit_body_after_validation(&self, body: String) -> Result<String, String> {
        let action = self
            .rules
            .lock()
            .map_err(|_| "form rules are unavailable".to_string())?
            .action
            .clone();
        let mut state = self.state.get();
        state.status = JetWebFormStatus::Submitting;
        state.error.clear();
        self.set_state(state, "submit-start");
        let result = match action {
            Some(action) => action(body),
            None => Ok(body),
        };
        match result {
            Ok(result) if result.len() <= JET_WEB_FORM_MAX_VALUE => {
                let mut state = self.state.get();
                state.status = JetWebFormStatus::Submitted;
                state.result = result.clone();
                state.error.clear();
                state.submission_count = state.submission_count.saturating_add(1);
                self.set_state(state, "submit-success");
                Ok(result)
            }
            Ok(_) => {
                let error = format!(
                    "form action result exceeds {} bytes",
                    JET_WEB_FORM_MAX_VALUE
                );
                let mut state = self.state.get();
                state.status = JetWebFormStatus::Error;
                state.error = error.clone();
                self.set_state(state, "submit-error");
                Err(error)
            }
            Err(error) => {
                let error = jet_web_form_bound_error(error);
                let mut state = self.state.get();
                state.status = JetWebFormStatus::Error;
                state.error = error.clone();
                self.set_state(state, "submit-error");
                Err(error)
            }
        }
    }

    /// The same action and encoding used by `submit`, exposed for a browser
    /// renderer that progressively enhances a normal POST form.
    pub fn no_script_submission(&self) -> Result<String, String> {
        self.validate()?;
        self.encode_submission()
    }

    pub fn html(&self) -> String {
        let state = self.state.get();
        let mut html = String::new();
        html.push_str("<form method=\"");
        html.push_str(&jet_web_form_escape(&state.method));
        html.push_str("\" action=\"");
        html.push_str(&jet_web_form_escape(&state.action));
        html.push_str("\">");
        for field in state.fields.values() {
            html.push_str("<label for=\"");
            html.push_str(&jet_web_form_escape(&field.name));
            html.push_str("\">");
            html.push_str(&jet_web_form_escape(&field.name));
            html.push_str("</label><input name=\"");
            html.push_str(&jet_web_form_escape(&field.name));
            html.push_str("\" id=\"");
            html.push_str(&jet_web_form_escape(&field.name));
            html.push_str("\" type=\"");
            html.push_str(jet_web_form_input_type(field.value_type));
            html.push('"');
            if field.required {
                html.push_str(" required");
            }
            html.push_str(" value=\"");
            html.push_str(&jet_web_form_escape(&field.value));
            html.push_str("\">");
            for error in &field.errors {
                html.push_str("<span class=\"error\">");
                html.push_str(&jet_web_form_escape(error));
                html.push_str("</span>");
            }
        }
        html.push_str("<button type=\"submit\">Submit</button></form>");
        html
    }

    pub fn show(&self) -> String {
        let state = self.state.get();
        let invalid = state.fields.values().filter(|field| !field.errors.is_empty()).count();
        format!(
            "Forms(status={},fields={},invalid={},action={},submissions={},error={})",
            state.status.name(),
            state.fields.len(),
            invalid,
            state.action,
            state.submission_count,
            state.error
        )
    }

    fn validate_one(
        &self,
        name: String,
        value: String,
        value_type: JetWebFormValueType,
        required: bool,
        run_async: bool,
    ) -> Result<(), String> {
        let rules = self.rules.lock().map_err(|_| "form rules are unavailable".to_string())?.clone();
        let mut errors = jet_web_form_validate_value(&value, value_type, required);
        if let Some(validators) = rules.validators.get(&name) {
            for validator in validators {
                if let Err(error) = validator(value.clone()) {
                    errors.push(jet_web_form_bound_error(error));
                }
            }
        }
        self.apply_one_errors(&name, errors.clone(), false);
        if run_async {
            if let Some(validators) = rules.async_validators.get(&name) {
                let validators = validators.clone();
                let form = self.clone();
                let name_for_worker = name.clone();
                let sync_errors = errors.clone();
                form.apply_one_errors(&name, sync_errors.clone(), true);
                let generation = form.validation_generation.load(JetWebFormOrdering::Acquire);
                let value_for_worker = value.clone();
                let task = jet_std::JetTask::spawn(move || {
                    let mut async_errors = sync_errors;
                    for validator in validators {
                        if let Err(error) = validator(value_for_worker.clone()) {
                            async_errors.push(jet_web_form_bound_error(error));
                        }
                    }
                    let current = form
                        .state
                        .get()
                        .fields
                        .get(&name_for_worker)
                        .map(|field| field.value.clone());
                    if form.validation_generation.load(JetWebFormOrdering::Acquire) == generation
                        && current.as_deref() == Some(value_for_worker.as_str())
                    {
                        form.apply_one_errors(&name_for_worker, async_errors, false);
                    }
                });
                task.detach();
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// Cancel pending asynchronous validation.  Workers are cooperative: a
    /// cancelled generation may finish its sleep or callback, but its result
    /// can no longer write into this form.
    pub fn cancel_validation(&self) {
        self.validation_generation.fetch_add(1, JetWebFormOrdering::AcqRel);
        let mut state = self.state.get();
        for field in state.fields.values_mut() {
            field.validating = false;
        }
        if state.status == JetWebFormStatus::Validating {
            state.status = JetWebFormStatus::Dirty;
        }
        self.set_state(state, "validation-cancelled");
    }

    /// Apply server-side field errors after the same decoder accepted a POST.
    /// Unknown fields stay form-level errors instead of being silently dropped.
    pub fn set_server_errors(
        &self,
        field_errors: JetWebFormMap<String, Vec<String>>,
        form_errors: Vec<String>,
    ) {
        let mut state = self.state.get();
        for field in state.fields.values_mut() {
            field.errors.clear();
        }
        let mut summary = Vec::new();
        for (name, errors) in field_errors {
            if let Some(field) = state.fields.get_mut(&name) {
                field.errors = errors
                    .into_iter()
                    .map(jet_web_form_bound_error)
                    .collect();
                field.touched = true;
                summary.extend(field.errors.iter().cloned());
            } else {
                let name = jet_web_form_bound_error(name);
                summary.push(format!("server returned an unknown field `{name}`"));
            }
        }
        summary.extend(form_errors.into_iter().map(jet_web_form_bound_error));
        state.status = JetWebFormStatus::Error;
        state.error = if summary.is_empty() {
            "form action failed".to_string()
        } else {
            jet_web_form_bound_error(summary.join("; "))
        };
        self.set_state(state, "server-errors");
    }

    pub fn field_state(&self, name: &str) -> Result<JetWebFormFieldState, String> {
        self.state
            .get()
            .fields
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown form field `{name}`"))
    }

    fn apply_one_errors(&self, name: &str, errors: Vec<String>, validating: bool) {
        let mut state = self.state.get();
        if let Some(field) = state.fields.get_mut(name) {
            field.errors = errors;
            field.validating = validating;
        }
        state.status = if validating {
            JetWebFormStatus::Validating
        } else if state.fields.values().any(|field| !field.errors.is_empty()) {
            JetWebFormStatus::Invalid
        } else {
            JetWebFormStatus::Dirty
        };
        self.set_state(state, "field-validation");
    }


    fn validation_current(&self, generation: u64, snapshot: &JetWebFormState) -> bool {
        if self.validation_generation.load(JetWebFormOrdering::Acquire) != generation {
            return false;
        }
        let current = self.state.get();
        snapshot.fields.iter().all(|(name, field)| {
            current
                .fields
                .get(name)
                .is_some_and(|candidate| candidate.value == field.value)
        })
    }

    fn apply_validation_if_current(
        &self,
        errors: JetWebFormMap<String, Vec<String>>,
        generation: u64,
    ) -> Result<(), String> {
        let snapshot = self.state.get();
        if !self.validation_current(generation, &snapshot) {
            return Ok(());
        }
        self.apply_validation(errors)
    }

    fn apply_validation(&self, errors: JetWebFormMap<String, Vec<String>>) -> Result<(), String> {
        let mut state = self.state.get();
        for field in state.fields.values_mut() {
            field.touched = true;
            field.validating = false;
            field.errors = errors.get(&field.name).cloned().unwrap_or_default();
        }
        if errors.is_empty() {
            state.status = JetWebFormStatus::Dirty;
            state.error.clear();
            self.set_state(state, "validation-success");
            Ok(())
        } else {
            state.status = JetWebFormStatus::Invalid;
            state.error = errors
                .values()
                .flat_map(|items| items.iter())
                .cloned()
                .collect::<Vec<_>>()
                .join("; ");
            self.set_state(state, "validation-error");
            Err(self.state.get().error)
        }
    }

    fn encode_submission(&self) -> Result<String, String> {
        let state = self.state.get();
        let mut body = String::new();
        for (index, field) in state.fields.values().enumerate() {
            if index > 0 {
                body.push('&');
            }
            body.push_str(&jet_web_form_encode(&field.name));
            body.push('=');
            body.push_str(&jet_web_form_encode(&field.value));
            if body.len() > JET_WEB_FORM_MAX_BODY {
                return Err(format!(
                    "encoded form body exceeds {} bytes",
                    JET_WEB_FORM_MAX_BODY
                ));
            }
        }
        Ok(body)
}
}

impl Default for JetWebForm {
    fn default() -> Self {
        Self::new(String::new())
    }
}

fn jet_web_form_input_type(value_type: JetWebFormValueType) -> &'static str {
    match value_type {
        JetWebFormValueType::String => "text",
        JetWebFormValueType::Int => "number",
        JetWebFormValueType::Bool => "checkbox",
        JetWebFormValueType::Float => "number",
    }
}

fn jet_web_form_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn jet_web_form_validate_value(
    value: &str,
    value_type: JetWebFormValueType,
    required: bool,
) -> Vec<String> {
    let mut errors = Vec::new();
    if required && value.trim().is_empty() {
        errors.push("value is required".to_string());
        return errors;
    }
    if value.trim().is_empty() {
        return errors;
    }
    match value_type {
        JetWebFormValueType::String => {}
        JetWebFormValueType::Int => {
            if value.parse::<i64>().is_err() {
                errors.push("expected Int".to_string());
            }
        }
        JetWebFormValueType::Bool => {
            if !matches!(value, "true" | "false" | "1" | "0") {
                errors.push("expected Bool".to_string());
            }
        }
        JetWebFormValueType::Float => match value.parse::<f64>() {
            Ok(number) if number.is_finite() => {}
            _ => errors.push("expected finite Float".to_string()),
        },
    }
    errors
}

fn jet_web_form_validate_field(
    field: &JetWebFormFieldState,
    validators: Option<&Vec<JetWebFormValidator>>,
) -> Vec<String> {
    let mut errors = jet_web_form_validate_value(&field.value, field.value_type, field.required);
    if let Some(validators) = validators {
        for validator in validators {
            if let Err(error) = validator(field.value.clone()) {
                errors.push(jet_web_form_bound_error(error));
            }
        }
    }
    errors
}

fn jet_web_form_bound_error(error: String) -> String {
    if error.len() <= JET_WEB_FORM_MAX_ERROR {
        return error;
    }
    error
        .char_indices()
        .take_while(|(index, _)| *index < JET_WEB_FORM_MAX_ERROR)
        .map(|(_, character)| character)
        .collect()
}

fn jet_web_form_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else if byte == b' ' {
            out.push('+');
        } else {
            out.push('%');
            out.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
            out.push(char::from(b"0123456789ABCDEF"[(byte & 0x0f) as usize]));
        }
    }
    out
}

fn jet_web_form_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(character),
        }
    }
    out
}

pub fn jet_web_forms_new(action: String) -> JetWebForm {
    JetWebForm::new(action)
}

pub fn jet_web_forms_field(
    form: &JetWebForm,
    name: String,
    value_type: String,
    required: bool,
) -> Result<JetWebForm, String> {
    let value_type = match value_type.as_str() {
        "String" | "string" => JetWebFormValueType::String,
        "Int" | "int" => JetWebFormValueType::Int,
        "Bool" | "bool" => JetWebFormValueType::Bool,
        "Float" | "float" => JetWebFormValueType::Float,
        _ => return Err(format!("unknown form field type `{value_type}`")),
    };
    form.field(name, value_type, required)
}

pub fn jet_web_forms_set(
    form: &JetWebForm,
    name: String,
    value: String,
) -> Result<(), String> {
    form.set(name, value)
}

pub fn jet_web_forms_blur(form: &JetWebForm, name: String) -> Result<(), String> {
    form.blur(name)
}

pub fn jet_web_forms_validate(form: &JetWebForm) -> Result<(), String> {
    form.validate()
}

pub fn jet_web_forms_validate_async(form: &JetWebForm) -> JetWebForm {
    let validation = form.validate_async();
    let _ = validation.wait();
    form.clone()
}

pub fn jet_web_forms_submit(form: &JetWebForm) -> Result<String, String> {
    form.submit()
}

pub fn jet_web_forms_no_script(form: &JetWebForm) -> Result<String, String> {
    form.no_script_submission()
}

pub fn jet_web_forms_html(form: &JetWebForm) -> String {
    form.html()
}

pub fn jet_web_forms_show(form: &JetWebForm) -> String {
    form.show()
}
/// Controls are deliberately closed.  Applications own markup, but a derived
/// form may only select controls whose escaping and parser contract is known.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebFormControl {
    Text,
    Email,
    Url,
    Password,
    Number,
    Date,
    Checkbox,
    Hidden,
}

impl JetWebFormControl {
    fn input_type(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Email => "email",
            Self::Url => "url",
            Self::Password => "password",
            Self::Number => "number",
            Self::Date => "date",
            Self::Checkbox => "checkbox",
            Self::Hidden => "hidden",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebFormValidationTiming {
    Change,
    Blur,
    Submit,
}

impl JetWebFormValidationTiming {
    pub fn name(self) -> &'static str {
        match self {
            Self::Change => "change",
            Self::Blur => "blur",
            Self::Submit => "submit",
        }
    }
}

type JetWebFormFieldParser =
    JetWebFormArc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

/// The field contract emitted by the typed-form front end.  `name` is the
/// source-struct field; `wire_name` is the only browser-facing spelling.
/// Keeping both makes renames explicit instead of changing the action input.
pub struct JetWebFormFieldSpec {
    pub name: String,
    pub value_type: JetWebFormValueType,
    pub required: bool,
    pub default: Option<String>,
    pub label: String,
    pub control: JetWebFormControl,
    pub group: Option<String>,
    pub wire_name: String,
    parser: Option<JetWebFormFieldParser>,
}

impl Clone for JetWebFormFieldSpec {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            value_type: self.value_type,
            required: self.required,
            default: self.default.clone(),
            label: self.label.clone(),
            control: self.control,
            group: self.group.clone(),
            wire_name: self.wire_name.clone(),
            parser: self.parser.clone(),
        }
    }
}

impl JetWebFormFieldSpec {
    pub fn new(
        name: String,
        value_type: JetWebFormValueType,
        required: bool,
    ) -> Result<Self, String> {
        if !jet_web_form_valid_name(&name) {
            return Err(format!("invalid form field name `{name}`"));
        }
        Ok(Self {
            name: name.clone(),
            value_type,
            required,
            default: None,
            label: jet_web_form_human_label(&name),
            control: match value_type {
                JetWebFormValueType::String => JetWebFormControl::Text,
                JetWebFormValueType::Int | JetWebFormValueType::Float => {
                    JetWebFormControl::Number
                }
                JetWebFormValueType::Bool => JetWebFormControl::Checkbox,
            },
            group: None,
            wire_name: name,
            parser: None,
        })
    }

    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    pub fn default_value(mut self, value: String) -> Result<Self, String> {
        if value.len() > JET_WEB_FORM_MAX_VALUE {
            return Err(format!(
                "default for form field `{}` exceeds {} bytes",
                self.name, JET_WEB_FORM_MAX_VALUE
            ));
        }
        self.default = Some(value);
        Ok(self)
    }

    pub fn with_label(mut self, label: String) -> Result<Self, String> {
        jet_web_form_validate_text(&label, "form field label")?;
        self.label = label;
        Ok(self)
    }

    pub fn with_control(mut self, control: JetWebFormControl) -> Self {
        self.control = control;
        self
    }

    pub fn in_group(mut self, group: String) -> Result<Self, String> {
        jet_web_form_validate_text(&group, "form field group")?;
        self.group = Some(group);
        Ok(self)
    }

    pub fn with_wire_name(mut self, wire_name: String) -> Result<Self, String> {
        if !jet_web_form_valid_name(&wire_name) {
            return Err(format!("invalid form wire name `{wire_name}`"));
        }
        self.wire_name = wire_name;
        Ok(self)
    }

    pub fn with_parser<F>(mut self, parser: F) -> Self
    where
        F: Fn(&str) -> Result<String, String> + Send + Sync + 'static,
    {
        self.parser = Some(JetWebFormArc::new(parser));
        self
    }

    fn parse(&self, raw: &str) -> Result<String, String> {
        if raw.len() > JET_WEB_FORM_MAX_VALUE {
            return Err(format!(
                "form field `{}` exceeds {} bytes",
                self.name, JET_WEB_FORM_MAX_VALUE
            ));
        }
        if self.required && raw.trim().is_empty() {
            return Err("value is required".to_string());
        }
        if raw.trim().is_empty() {
            return Ok(String::new());
        }
        let parsed = if let Some(parser) = &self.parser {
            parser(raw)
        } else {
            match self.value_type {
                JetWebFormValueType::String => Ok(raw.to_string()),
                JetWebFormValueType::Int => raw
                    .parse::<i64>()
                    .map(|value| value.to_string())
                    .map_err(|_| "expected Int".to_string()),
                JetWebFormValueType::Bool => match raw {
                    "true" | "1" => Ok("true".to_string()),
                    "false" | "0" => Ok("false".to_string()),
                    _ => Err("expected Bool".to_string()),
                },
                JetWebFormValueType::Float => {
                    let value = raw
                        .parse::<f64>()
                        .map_err(|_| "expected Float".to_string())?;
                    if value.is_finite() {
                        Ok(value.to_string())
                    } else {
                        Err("expected finite Float".to_string())
                    }
                }
            }
        }?;
        if parsed.len() > JET_WEB_FORM_MAX_VALUE {
            return Err(format!(
                "parsed form field `{}` exceeds {} bytes",
                self.name, JET_WEB_FORM_MAX_VALUE
            ));
        }
        if parsed.chars().any(char::is_control) {
            return Err(format!("form field `{}` contains a control character", self.name));
        }
        Ok(parsed)
    }
}

#[derive(Clone)]
pub struct JetWebFormInput {
    type_name: String,
    fields: Vec<JetWebFormFieldSpec>,
    excluded: std::collections::BTreeSet<String>,
}

impl JetWebFormInput {
    /// The only constructor for a form input.  The compiler emits this for a
    /// dedicated action-input struct; there is intentionally no record-editing
    /// constructor.
    pub fn dedicated(
        type_name: String,
        fields: Vec<JetWebFormFieldSpec>,
    ) -> Result<Self, String> {
        let input = Self {
            type_name,
            fields,
            excluded: std::collections::BTreeSet::new(),
        };
        input.validate()?;
        Ok(input)
    }

    pub fn new(type_name: String, fields: Vec<JetWebFormFieldSpec>) -> Result<Self, String> {
        Self::dedicated(type_name, fields)
    }

    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    pub fn fields(&self) -> Vec<JetWebFormFieldSpec> {
        self.fields
            .iter()
            .filter(|field| !self.excluded.contains(&field.name))
            .cloned()
            .collect()
    }

    pub fn rename(mut self, name: String, wire_name: String) -> Result<Self, String> {
        let field_index = self
            .fields
            .iter()
            .position(|field| field.name == name)
            .ok_or_else(|| format!("unknown form field `{name}`"))?;
        if !jet_web_form_valid_name(&wire_name) {
            return Err(format!("invalid form wire name `{wire_name}`"));
        }
        if self
            .fields
            .iter()
            .enumerate()
            .any(|(index, other)| index != field_index && other.wire_name == wire_name)
        {
            return Err(format!("duplicate form wire name `{wire_name}`"));
        }
        self.fields[field_index].wire_name = wire_name;
        self.validate()?;
        Ok(self)
    }

    pub fn exclude(mut self, name: String) -> Result<Self, String> {
        if !self.fields.iter().any(|field| field.name == name) {
            return Err(format!("unknown form field `{name}`"));
        }
        self.excluded.insert(name);
        self.validate()?;
        Ok(self)
    }

    pub fn group(mut self, name: String, group: String) -> Result<Self, String> {
        jet_web_form_validate_text(&group, "form field group")?;
        let field = self
            .fields
            .iter_mut()
            .find(|field| field.name == name)
            .ok_or_else(|| format!("unknown form field `{name}`"))?;
        field.group = Some(group);
        self.validate()?;
        Ok(self)
    }

    pub fn replace(
        mut self,
        name: String,
        mut replacement: JetWebFormFieldSpec,
    ) -> Result<Self, String> {
        let index = self
            .fields
            .iter()
            .position(|field| field.name == name)
            .ok_or_else(|| format!("unknown form field `{name}`"))?;
        if replacement.name != name {
            return Err(format!(
                "replacement field `{}` must retain source name `{name}`",
                replacement.name
            ));
        }
        if replacement.wire_name == name {
            replacement.wire_name = self.fields[index].wire_name.clone();
        }
        self.fields[index] = replacement;
        self.validate()?;
        Ok(self)
    }

    pub fn form(&self, action: String) -> Result<JetWebFormTyped, String> {
        JetWebFormTyped::from_input(self, action, None)
    }

    pub fn form_with_action<F>(
        &self,
        action: String,
        handler: F,
    ) -> Result<JetWebFormTyped, String>
    where
        F: Fn(JetWebFormDecodedInput) -> Result<String, JetWebFormActionError>
            + Send
            + Sync
            + 'static,
    {
        JetWebFormTyped::from_input(self, action, Some(JetWebFormArc::new(handler)))
    }

    pub fn decode_post(&self, body: &str) -> Result<JetWebFormDecodedInput, String> {
        self.validate()?;
        if body.len() > JET_WEB_FORM_MAX_BODY {
            return Err(format!(
                "encoded form body exceeds {} bytes",
                JET_WEB_FORM_MAX_BODY
            ));
        }
        let mut posted = JetWebFormMap::new();
        for pair in body.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
            let key = jet_web_form_decode_component(raw_key)?;
            let value = jet_web_form_decode_component(raw_value)?;
            if !jet_web_form_valid_name(&key) {
                return Err(format!("invalid form field name `{key}`"));
            }
            if posted.insert(key.clone(), value).is_some() {
                return Err(format!("form field `{key}` was posted more than once"));
            }
        }
        for field in &self.fields {
            if self.excluded.contains(&field.name)
                && posted.contains_key(&field.wire_name)
            {
                return Err(format!("form field `{}` is excluded", field.name));
            }
        }
        let mut values = JetWebFormMap::new();
        let mut wire_values = JetWebFormMap::new();
        for field in self.fields() {
            let raw = match posted.remove(&field.wire_name) {
                Some(value) => value,
                None if field.default.is_some() => field.default.clone().unwrap_or_default(),
                None if field.value_type == JetWebFormValueType::Bool && !field.required => {
                    "false".to_string()
                }
                None if field.required => {
                    return Err(format!("missing required form field `{}`", field.name));
                }
                None => String::new(),
            };
            let parsed = field
                .parse(&raw)
                .map_err(|error| format!("field `{}`: {error}", field.name))?;
            values.insert(field.name.clone(), parsed);
            wire_values.insert(field.wire_name.clone(), raw);
        }
        if let Some((name, _)) = posted.into_iter().next() {
            return Err(format!("unknown form field `{name}`"));
        }
        Ok(JetWebFormDecodedInput {
            type_name: self.type_name.clone(),
            values,
            wire_values,
        })
    }

    fn encode_post(&self, input: &JetWebFormDecodedInput) -> Result<String, String> {
        if input.type_name != self.type_name {
            return Err(format!(
                "form input type `{}` does not match `{}`",
                input.type_name, self.type_name
            ));
        }
        let mut body = String::new();
        for (index, field) in self.fields().into_iter().enumerate() {
            let value = input
                .values
                .get(&field.name)
                .cloned()
                .or_else(|| field.default.clone())
                .unwrap_or_else(|| {
                    if field.value_type == JetWebFormValueType::Bool {
                        "false".to_string()
                    } else {
                        String::new()
                    }
                });
            let value = field
                .parse(&value)
                .map_err(|error| format!("field `{}`: {error}", field.name))?;
            if index > 0 {
                body.push('&');
            }
            body.push_str(&jet_web_form_encode(&field.wire_name));
            body.push('=');
            body.push_str(&jet_web_form_encode(&value));
            if body.len() > JET_WEB_FORM_MAX_BODY {
                return Err(format!(
                    "encoded form body exceeds {} bytes",
                    JET_WEB_FORM_MAX_BODY
                ));
            }
        }
        Ok(body)
    }

    fn validate(&self) -> Result<(), String> {
        if !jet_web_form_valid_type_name(&self.type_name) {
            return Err(format!("invalid dedicated form input type `{}`", self.type_name));
        }
        if self.fields.is_empty() {
            return Err(format!("dedicated form input `{}` has no fields", self.type_name));
        }
        if self.fields.len() > JET_WEB_FORM_MAX_FIELDS {
            return Err("form input has too many fields".to_string());
        }
        let mut names = std::collections::BTreeSet::new();
        let mut wire_names = std::collections::BTreeSet::new();
        for field in &self.fields {
            if !jet_web_form_valid_name(&field.name)
                || !names.insert(field.name.clone())
            {
                return Err(format!("duplicate or invalid form field `{}`", field.name));
            }
            if !jet_web_form_valid_name(&field.wire_name)
                || !wire_names.insert(field.wire_name.clone())
            {
                return Err(format!(
                    "duplicate or invalid form wire name `{}`",
                    field.wire_name
                ));
            }
            jet_web_form_validate_text(&field.label, "form field label")?;
            if let Some(group) = &field.group {
                jet_web_form_validate_text(group, "form field group")?;
            }
            if let Some(default) = &field.default {
                field.parse(default).map_err(|error| {
                    format!("default for form field `{}`: {error}", field.name)
                })?;
            }
        }
        if self.excluded.iter().any(|name| !names.contains(name)) {
            return Err("form excludes an unknown field".to_string());
        }
        if self.fields().is_empty() {
            return Err(format!("dedicated form input `{}` excludes every field", self.type_name));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct JetWebFormDecodedInput {
    pub type_name: String,
    pub values: JetWebFormMap<String, String>,
    pub wire_values: JetWebFormMap<String, String>,
}

impl JetWebFormDecodedInput {
    pub fn value(&self, name: &str) -> Option<String> {
        self.values.get(name).cloned()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JetWebFormActionError {
    pub field_errors: JetWebFormMap<String, Vec<String>>,
    pub form_errors: Vec<String>,
}

impl JetWebFormActionError {
    pub fn field(mut self, name: String, message: String) -> Self {
        if !jet_web_form_valid_name(&name) || name.len() > JET_WEB_FORM_MAX_NAME {
            self.form_errors
                .push("action returned an invalid field error".to_string());
            return self;
        }
        self.field_errors
            .entry(name)
            .or_default()
            .push(jet_web_form_bound_error(message));
        self
    }

    pub fn form(mut self, message: String) -> Self {
        self.form_errors.push(jet_web_form_bound_error(message));
        self
    }

    fn normalized(&self) -> Self {
        let mut normalized = Self::default();
        for (name, messages) in &self.field_errors {
            for message in messages {
                normalized = normalized.field(name.clone(), message.clone());
            }
        }
        for message in &self.form_errors {
            normalized = normalized.form(message.clone());
        }
        normalized
    }

    fn summary(&self) -> String {
        let normalized = self.normalized();
        let mut messages = Vec::new();
        for (name, errors) in &normalized.field_errors {
            for error in errors {
                messages.push(format!("{name}: {error}"));
            }
        }
        messages.extend(normalized.form_errors.iter().cloned());
        if messages.is_empty() {
            "form action failed".to_string()
        } else {
            jet_web_form_bound_error(messages.join("; "))
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetWebFormLifecycleStatus {
    Idle,
    Pending,
    Submitting,
    Success,
    Failure,
    Cancelled,
}

impl JetWebFormLifecycleStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Pending => "pending",
            Self::Submitting => "submitting",
            Self::Success => "success",
            Self::Failure => "failure",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetWebFormLifecycle {
    pub status: JetWebFormLifecycleStatus,
    pub result: String,
    pub error: String,
    pub generation: u64,
}

impl JetWebFormLifecycle {
    fn idle() -> Self {
        Self {
            status: JetWebFormLifecycleStatus::Idle,
            result: String::new(),
            error: String::new(),
            generation: 0,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JetWebFormErrorState {
    pub fields: JetWebFormMap<String, Vec<String>>,
    pub form: Vec<String>,
}

pub type JetWebFormTypedAction =
    JetWebFormArc<dyn Fn(JetWebFormDecodedInput) -> Result<String, JetWebFormActionError> + Send + Sync>;

#[derive(Clone)]
pub struct JetWebFormTyped {
    input: JetWebFormArc<JetWebFormInput>,
    form: JetWebForm,
    lifecycle: jet_std::JetSignal<JetWebFormLifecycle>,
    focus: jet_std::JetSignal<Option<String>>,
    action_errors: JetWebFormArc<JetWebFormMutex<Option<JetWebFormActionError>>>,
    cancelled: JetWebFormArc<std::sync::atomic::AtomicBool>,
    async_timings: JetWebFormArc<JetWebFormMutex<JetWebFormMap<String, JetWebFormValidationTiming>>>,
}

impl JetWebFormTyped {
    fn from_input(
        input: &JetWebFormInput,
        action: String,
        handler: Option<JetWebFormTypedAction>,
    ) -> Result<Self, String> {
        input.validate()?;
        let input = JetWebFormArc::new(input.clone());
        let form = JetWebForm::new(action);
        for field in input.fields() {
            let name = field.name.clone();
            form.field(name.clone(), field.value_type, field.required)?;
            if let Some(default) = field.default.clone() {
                form.set(name.clone(), default)?;
            }
            if let Some(parser) = field.parser.clone() {
                let parser_name = name.clone();
                form.set_validator(name, move |value| {
                    parser(value.as_str())
                        .map(|_| ())
                        .map_err(|error| format!("field `{parser_name}`: {error}"))
                })?;
            }
        }
        let mut initial = form.state.get();
        initial.status = JetWebFormStatus::Idle;
        initial.error.clear();
        initial.result.clear();
        form.state.set(initial);

        let action_errors = JetWebFormArc::new(JetWebFormMutex::new(None));
        let typed = Self {
            input,
            form,
            lifecycle: jet_std::JetSignal::new(JetWebFormLifecycle::idle()),
            focus: jet_std::JetSignal::new(None),
            action_errors,
            cancelled: JetWebFormArc::new(std::sync::atomic::AtomicBool::new(false)),
            async_timings: JetWebFormArc::new(JetWebFormMutex::new(JetWebFormMap::new())),
        };
        jet_web_form_register_panel_fact(&typed);
        if let Some(handler) = handler {
            typed.install_action(handler);
        }
        Ok(typed)
    }

    pub fn input(&self) -> JetWebFormInput {
        (*self.input).clone()
    }

    pub fn fields(&self) -> Vec<JetWebFormFieldSpec> {
        self.input.fields()
    }

    pub fn state(&self) -> JetWebFormState {
        self.form.state()
    }

    pub fn state_signal(&self) -> jet_std::JetSignal<JetWebFormState> {
        self.form.state_signal()
    }

    pub fn lifecycle(&self) -> JetWebFormLifecycle {
        self.lifecycle.get()
    }

    pub fn lifecycle_signal(&self) -> jet_std::JetSignal<JetWebFormLifecycle> {
        self.lifecycle.clone()
    }

    pub fn focus(&self) -> Option<String> {
        self.focus.get()
    }

    pub fn focus_signal(&self) -> jet_std::JetSignal<Option<String>> {
        self.focus.clone()
    }

    pub fn set(&self, name: String, value: String) -> Result<(), String> {
        self.form.set(name.clone(), value)?;
        let timing = self
            .async_timings
            .lock()
            .ok()
            .and_then(|timings| timings.get(&name).copied());
        if timing == Some(JetWebFormValidationTiming::Change) {
            if let Ok(field) = self.form.field_state(&name) {
                let _ = self.form.validate_one(
                    name,
                    field.value,
                    field.value_type,
                    field.required,
                    true,
                );
            }
        }
        Ok(())
    }

    pub fn blur(&self, name: String) -> Result<(), String> {
        let run_async = self
            .async_timings
            .lock()
            .ok()
            .and_then(|timings| timings.get(&name).copied())
            .map(|timing| timing == JetWebFormValidationTiming::Blur)
            .unwrap_or(true);
        self.form.blur_with_async(name, run_async)
    }

    pub fn set_validator<F>(&self, name: String, validator: F) -> Result<(), String>
    where
        F: Fn(String) -> Result<(), String> + Send + Sync + 'static,
    {
        self.form.set_validator(name, validator)
    }
    pub fn set_action<F>(&self, handler: F)
    where
        F: Fn(JetWebFormDecodedInput) -> Result<String, JetWebFormActionError>
            + Send
            + Sync
            + 'static,
    {
        self.set_action_handle(JetWebFormArc::new(handler));
    }

    pub fn set_action_handle(&self, handler: JetWebFormTypedAction) {
        self.install_action(handler);
    }

    fn install_action(&self, handler: JetWebFormTypedAction) {
        let input = self.input.clone();
        let action_errors = self.action_errors.clone();
        self.form.set_action(move |body| {
            if let Ok(mut slot) = action_errors.lock() {
                *slot = None;
            }
            let decoded = input.decode_post(&body)?;
            match handler(decoded) {
                Ok(result) => Ok(result),
                Err(error) => {
                    let error = error.normalized();
                    if let Ok(mut slot) = action_errors.lock() {
                        *slot = Some(error.clone());
                    }
                    Err(error.summary())
                }
            }
        });
    }


    pub fn set_async_validator<F>(
        &self,
        name: String,
        timing: JetWebFormValidationTiming,
        debounce_ms: u64,
        validator: F,
    ) -> Result<(), String>
    where
        F: Fn(String) -> Result<(), String> + Send + Sync + 'static,
    {
        self.form.field_state(&name)?;
        if let Ok(mut timings) = self.async_timings.lock() {
            timings.insert(name.clone(), timing);
        }
        let delay = debounce_ms.min(10_000);
        self.form.set_async_validator(name, move |value| {
            if delay > 0 {
                std::thread::sleep(std::time::Duration::from_millis(delay));
            }
            validator(value)
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        self.cancelled.store(false, JetWebFormOrdering::Release);
        self.set_lifecycle(JetWebFormLifecycleStatus::Pending, String::new(), String::new());
        let result = self.form.validate();
        match &result {
            Ok(()) => self.set_lifecycle(
                JetWebFormLifecycleStatus::Idle,
                String::new(),
                String::new(),
            ),
            Err(error) => self.set_lifecycle(
                JetWebFormLifecycleStatus::Failure,
                String::new(),
                error.clone(),
            ),
        }
        result
    }

    pub fn validate_async(&self) -> JetWebFormTypedValidation {
        self.cancelled.store(false, JetWebFormOrdering::Release);
        self.set_lifecycle(JetWebFormLifecycleStatus::Pending, String::new(), String::new());
        JetWebFormTypedValidation {
            validation: Some(self.form.validate_async()),
            form: self.clone(),
        }
    }

    pub fn submit(&self) -> Result<String, String> {
        if self.cancelled.swap(false, JetWebFormOrdering::AcqRel) {
            let error = "form submission was cancelled".to_string();
            self.set_lifecycle(
                JetWebFormLifecycleStatus::Cancelled,
                String::new(),
                error.clone(),
            );
            return Err(error);
        }
        self.set_lifecycle(JetWebFormLifecycleStatus::Pending, String::new(), String::new());
        let state = self.form.state();
        let mut values = JetWebFormMap::new();
        for field in self.input.fields() {
            let value = state
                .fields
                .get(&field.name)
                .map(|field| field.value.clone())
                .unwrap_or_default();
            values.insert(field.name, value);
        }
        let decoded = JetWebFormDecodedInput {
            type_name: self.input.type_name.clone(),
            values,
            wire_values: JetWebFormMap::new(),
        };
        let body = match self.input.encode_post(&decoded) {
            Ok(body) => body,
            Err(error) => {
                self.set_lifecycle(
                    JetWebFormLifecycleStatus::Failure,
                    String::new(),
                    error.clone(),
                );
                return Err(error);
            }
        };
        if self.cancelled.load(JetWebFormOrdering::Acquire) {
            let error = "form submission was cancelled".to_string();
            self.form.cancel_validation();
            self.set_lifecycle(
                JetWebFormLifecycleStatus::Cancelled,
                String::new(),
                error.clone(),
            );
            return Err(error);
        }
        self.set_lifecycle(
            JetWebFormLifecycleStatus::Submitting,
            String::new(),
            String::new(),
        );
        let result = self.form.submit_body(body);
        self.apply_action_errors();
        if self.cancelled.load(JetWebFormOrdering::Acquire) {
            let error = "form submission was cancelled".to_string();
            self.set_lifecycle(
                JetWebFormLifecycleStatus::Cancelled,
                String::new(),
                error.clone(),
            );
            return Err(error);
        }
        match &result {
            Ok(value) => self.set_lifecycle(
                JetWebFormLifecycleStatus::Success,
                value.clone(),
                String::new(),
            ),
            Err(error) => self.set_lifecycle(
                JetWebFormLifecycleStatus::Failure,
                String::new(),
                error.clone(),
            ),
        }
        result
    }

    pub fn submit_async(&self) -> JetWebFormTypedSubmission {
        self.cancelled.store(false, JetWebFormOrdering::Release);
        JetWebFormTypedSubmission {
            join: Some(jet_std::JetTask::spawn({
                let form = self.clone();
                move || form.submit()
            })),
            form: self.clone(),
        }
    }

    pub fn no_script_submission(&self) -> Result<String, String> {
        self.validate()?;
        let state = self.form.state();
        let values = self
            .input
            .fields()
            .into_iter()
            .map(|field| {
                let value = state
                    .fields
                    .get(&field.name)
                    .map(|field| field.value.clone())
                    .unwrap_or_default();
                (field.name, value)
            })
            .collect();
        self.input.encode_post(&JetWebFormDecodedInput {
            type_name: self.input.type_name.clone(),
            values,
            wire_values: JetWebFormMap::new(),
        })
    }

    pub fn post(&self, body: String) -> Result<String, String> {
        let decoded = self.input.decode_post(&body)?;
        for field in self.input.fields() {
            if let Some(value) = decoded.values.get(&field.name) {
                self.form.set(field.name.clone(), value.clone())?;
            }
        }
        self.submit()
    }

    pub fn decode_post(&self, body: &str) -> Result<JetWebFormDecodedInput, String> {
        self.input.decode_post(body)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, JetWebFormOrdering::Release);
        self.form.cancel_validation();
        self.set_lifecycle(
            JetWebFormLifecycleStatus::Cancelled,
            String::new(),
            "form operation was cancelled".to_string(),
        );
    }

    pub fn focus_field(&self, name: String) -> Result<(), String> {
        let field = self
            .input
            .fields()
            .into_iter()
            .find(|field| field.name == name)
            .ok_or_else(|| format!("unknown form field `{name}`"))?;
        self.focus.set(Some(field.name));
        Ok(())
    }

    pub fn field_state(&self, name: String) -> Result<JetWebFormFieldState, String> {
        self.form.field_state(&name)
    }

    pub fn select_field(
        &self,
        name: String,
    ) -> Result<jet_std::JetDerived<JetWebFormFieldState>, String> {
        let fallback = self.form.field_state(&name)?;
        let source = self.form.state_signal();
        Ok(jet_std::JetDerived::new_distinct(move || {
            source
                .get()
                .fields
                .get(&name)
                .cloned()
                .unwrap_or_else(|| fallback.clone())
        }))
    }

    pub fn errors(&self) -> JetWebFormErrorState {
        let state = self.form.state();
        let fields = state
            .fields
            .iter()
            .filter(|(_, field)| !field.errors.is_empty())
            .map(|(name, field)| (name.clone(), field.errors.clone()))
            .collect();
        let form = if state.error.is_empty() {
            Vec::new()
        } else {
            vec![state.error]
        };
        JetWebFormErrorState { fields, form }
    }

    pub fn render(&self) -> String {
        let state = self.form.state();
        let lifecycle = self.lifecycle();
        let busy = matches!(
            lifecycle.status,
            JetWebFormLifecycleStatus::Pending | JetWebFormLifecycleStatus::Submitting
        );
        let mut html = String::new();
        html.push_str("<form method=\"post\" action=\"");
        html.push_str(&jet_web_form_escape(&state.action));
        html.push_str("\" aria-busy=\"");
        html.push_str(if busy { "true" } else { "false" });
        html.push_str("\">");
        let mut open_group: Option<String> = None;
        for spec in self.input.fields() {
            if spec.group != open_group {
                if open_group.is_some() {
                    html.push_str("</fieldset>");
                }
                if let Some(group) = spec.group.as_ref() {
                    html.push_str("<fieldset><legend>");
                    html.push_str(&jet_web_form_escape(group));
                    html.push_str("</legend>");
                }
                open_group = spec.group.clone();
            }
            let Some(field) = state.fields.get(&spec.name) else {
                continue;
            };
            let id = jet_web_form_field_id(&self.input.type_name, &spec.wire_name);
            let error_id = format!("{id}-error");
            let invalid = !field.errors.is_empty();
            html.push_str("<div class=\"field\" data-field=\"");
            html.push_str(&jet_web_form_escape(&spec.name));
            html.push_str("\">");
            if spec.control != JetWebFormControl::Hidden {
                html.push_str("<label for=\"");
                html.push_str(&jet_web_form_escape(&id));
                html.push_str("\">");
                html.push_str(&jet_web_form_escape(&spec.label));
                html.push_str("</label>");
            }
            html.push_str("<input name=\"");
            html.push_str(&jet_web_form_escape(&spec.wire_name));
            html.push_str("\" id=\"");
            html.push_str(&jet_web_form_escape(&id));
            html.push_str("\" type=\"");
            html.push_str(spec.control.input_type());
            html.push_str("\" aria-label=\"");
            html.push_str(&jet_web_form_escape(&spec.label));
            html.push_str("\" aria-invalid=\"");
            html.push_str(if invalid { "true" } else { "false" });
            if invalid {
                html.push_str("\" aria-describedby=\"");
                html.push_str(&jet_web_form_escape(&error_id));
            }
            html.push('"');
            if spec.required {
                html.push_str(" required");
            }
            if self.focus.get().as_deref() == Some(spec.name.as_str()) {
                html.push_str(" autofocus");
            }
            if spec.control == JetWebFormControl::Checkbox {
                html.push_str(" value=\"true\"");
                if matches!(field.value.as_str(), "true" | "1") {
                    html.push_str(" checked");
                }
            } else {
                html.push_str(" value=\"");
                html.push_str(&jet_web_form_escape(&field.value));
                html.push('"');
            }
            html.push('>');
            if invalid {
                html.push_str("<span id=\"");
                html.push_str(&jet_web_form_escape(&error_id));
                html.push_str("\" role=\"alert\" class=\"error\">");
                html.push_str(&jet_web_form_escape(&field.errors.join("; ")));
                html.push_str("</span>");
            }
            html.push_str("</div>");
        }
        if open_group.is_some() {
            html.push_str("</fieldset>");
        }
        if !state.error.is_empty() {
            html.push_str("<div role=\"alert\" class=\"form-error\" aria-live=\"assertive\">");
            html.push_str(&jet_web_form_escape(&state.error));
            html.push_str("</div>");
        }
        html.push_str("<button type=\"submit\"");
        if busy {
            html.push_str(" disabled");
        }
        html.push_str(">Submit</button><output aria-live=\"polite\">");
        html.push_str(&jet_web_form_escape(lifecycle.status.name()));
        html.push_str("</output></form>");
        html
    }


    pub fn show(&self) -> String {
        let state = self.form.state();
        let invalid = state.fields.values().filter(|field| !field.errors.is_empty()).count();
        let lifecycle = self.lifecycle();
        format!(
            "TypedForm(type={},status={},lifecycle={},fields={},invalid={},submissions={},error={})",
            self.input.type_name,
            state.status.name(),
            lifecycle.status.name(),
            state.fields.len(),
            invalid,
            state.submission_count,
            state.error
        )
    }

    fn apply_action_errors(&self) {
        let error = self
            .action_errors
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some(error) = error {
            self.form.set_server_errors(error.field_errors, error.form_errors);
        }
    }

    fn set_lifecycle(
        &self,
        status: JetWebFormLifecycleStatus,
        result: String,
        error: String,
    ) {
        let generation = self.lifecycle.get().generation.saturating_add(1);
        let lifecycle = JetWebFormLifecycle {
            status,
            result,
            error,
            generation,
        };
        self.lifecycle.set(lifecycle.clone());
        jet_web_form_publish_lifecycle(&self.input.type_name, &lifecycle, &self.form.state());
    }
}

#[derive(Clone)]
pub struct JetWebFormValidationChain {
    form: JetWebFormTyped,
}

impl JetWebFormValidationChain {
    fn new(form: JetWebFormTyped) -> Self {
        Self { form }
    }

    fn validate<F>(
        &self,
        name: String,
        timing: JetWebFormValidationTiming,
        validator: F,
    ) -> Self
    where
        F: Fn(String) -> Result<(), String> + Send + Sync + 'static,
    {
        self.form
            .set_async_validator(name.clone(), timing, 0, validator)
            .unwrap_or_else(|error| {
                panic!("checked form validator field `{name}` is unavailable: {error}")
            });
        Self::new(self.form.clone())
    }

    fn render(&self) -> String {
        self.form.render()
    }
}

pub struct JetWebFormTypedValidation {
    validation: Option<JetWebFormValidation>,
    form: JetWebFormTyped,
}

impl JetWebFormTypedValidation {
    pub fn wait(mut self) -> Result<(), String> {
        let validation = self
            .validation
            .take()
            .ok_or_else(|| "form validation worker is unavailable".to_string())?;
        let result = validation.wait();
        if self.form.cancelled.load(JetWebFormOrdering::Acquire) {
            let error = "form validation was cancelled".to_string();
            self.form.set_lifecycle(
                JetWebFormLifecycleStatus::Cancelled,
                String::new(),
                error.clone(),
            );
            return Err(error);
        }
        match &result {
            Ok(()) => self.form.set_lifecycle(
                JetWebFormLifecycleStatus::Idle,
                String::new(),
                String::new(),
            ),
            Err(error) => self.form.set_lifecycle(
                JetWebFormLifecycleStatus::Failure,
                String::new(),
                error.clone(),
            ),
        }
        result
    }

    pub fn cancel(&self) {
        if let Some(validation) = &self.validation {
            validation.cancel();
        }
        self.form.cancel();
    }
}
pub struct JetWebFormTypedSubmission {
    join: Option<jet_std::JetTask<Result<String, String>>>,
    form: JetWebFormTyped,
}


impl JetWebFormTypedSubmission {
    pub fn wait(mut self) -> Result<String, String> {
        let join = self
            .join
            .take()
            .ok_or_else(|| "form submission worker is unavailable".to_string())?;
        let result = join.join();
        if self.form.cancelled.load(JetWebFormOrdering::Acquire) {
            let error = "form submission was cancelled".to_string();
            self.form.set_lifecycle(
                JetWebFormLifecycleStatus::Cancelled,
                String::new(),
                error.clone(),
            );
            return Err(error);
        }
        match result {
            Ok(result) => result,
            Err(error) => Err(format!("form submission task failed: {error:?}")),
        }
    }

    pub fn cancel(&self) {
        if let Some(join) = &self.join {
            join.cancel();
        }
        self.form.cancel();
    }
}


fn jet_web_form_validate_text(value: &str, kind: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{kind} must not be empty"));
    }
    if value.len() > JET_WEB_FORM_MAX_NAME {
        return Err(format!("{kind} exceeds {JET_WEB_FORM_MAX_NAME} bytes"));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{kind} contains a control character"));
    }
    Ok(())
}

fn jet_web_form_human_label(name: &str) -> String {
    let mut label = String::new();
    for (index, word) in name.split(['_', '-']).enumerate() {
        if word.is_empty() {
            continue;
        }
        if index > 0 {
            label.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            label.extend(first.to_uppercase());
            label.extend(chars);
        }
    }
    label
}

fn jet_web_form_valid_type_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .split("::")
            .all(jet_web_form_valid_name)
}

fn jet_web_form_field_id(type_name: &str, wire_name: &str) -> String {
    let mut id = String::with_capacity(type_name.len() + wire_name.len() + 1);
    for character in type_name.chars().chain(std::iter::once('-')).chain(wire_name.chars()) {
        if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
            id.push(character);
        } else {
            id.push('-');
        }
    }
    id
}

fn jet_web_form_decode_component(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => decoded.push(b' '),
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err("form body contains an incomplete percent escape".to_string());
                }
                let high = jet_web_form_hex(bytes[index + 1])
                    .ok_or_else(|| "form body contains an invalid percent escape".to_string())?;
                let low = jet_web_form_hex(bytes[index + 2])
                    .ok_or_else(|| "form body contains an invalid percent escape".to_string())?;
                decoded.push((high << 4) | low);
                index += 2;
            }
            byte => decoded.push(byte),
        }
        index += 1;
        if decoded.len() > JET_WEB_FORM_MAX_VALUE {
            return Err(format!(
                "decoded form value exceeds {} bytes",
                JET_WEB_FORM_MAX_VALUE
            ));
        }
    }
    let value = String::from_utf8(decoded)
        .map_err(|_| "form body contains invalid UTF-8".to_string())?;
    if value.chars().any(char::is_control) {
        return Err("form value contains a control character".to_string());
    }
    Ok(value)
}

fn jet_web_form_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
pub fn jet_web_forms_input(
    type_name: String,
    fields: Vec<JetWebFormFieldSpec>,
) -> Result<JetWebFormInput, String> {
    JetWebFormInput::dedicated(type_name, fields)
}

pub fn jet_web_forms_typed(
    input: &JetWebFormInput,
    action: String,
) -> Result<JetWebFormTyped, String> {
    let endpoint = if action.starts_with('/') || action.contains("://") {
        action
    } else {
        format!("/actions/{action}")
    };
    input.form(endpoint)
}

// The model-derived input has already passed `forms.input`. Its constructor
// returns a form, not the Result carrier used by runtime schema construction.
pub fn jet_web_form(input: &JetWebFormInput, action: String) -> JetWebFormTyped {
    jet_web_forms_typed(input, action).expect("validated web.form input")
}

pub fn jet_web_forms_typed_submit(form: &JetWebFormTyped) -> Result<String, String> {
    form.submit()
}

pub fn jet_web_forms_typed_no_script(
    form: &JetWebFormTyped,
) -> Result<String, String> {
    form.no_script_submission()
}

pub fn jet_web_forms_typed_html(form: &JetWebFormTyped) -> String {
    form.render()
}

pub fn jet_web_forms_typed_show(form: &JetWebFormTyped) -> String {
    form.show()
}

pub fn jet_web_forms_typed_state(form: &JetWebFormTyped) -> JetWebFormState {
    form.state()
}

pub fn jet_web_forms_typed_lifecycle(
    form: &JetWebFormTyped,
) -> JetWebFormLifecycle {
    form.lifecycle()
}

pub fn jet_web_forms_typed_errors(form: &JetWebFormTyped) -> JetWebFormErrorState {
    form.errors()
}

pub fn jet_web_forms_typed_focus(
    form: &JetWebFormTyped,
    name: String,
) -> Result<(), String> {
    form.focus_field(name)
}

pub fn jet_web_forms_typed_cancel(form: &JetWebFormTyped) {
    form.cancel();
}
pub fn jet_web_forms_input_rename(
    input: JetWebFormInput,
    name: String,
    wire_name: String,
) -> Result<JetWebFormInput, String> {
    input.rename(name, wire_name)
}

pub fn jet_web_forms_input_exclude(
    input: JetWebFormInput,
    name: String,
) -> Result<JetWebFormInput, String> {
    input.exclude(name)
}

pub fn jet_web_forms_input_group(
    input: JetWebFormInput,
    name: String,
    group: String,
) -> Result<JetWebFormInput, String> {
    input.group(name, group)
}

pub fn jet_web_forms_input_replace(
    input: JetWebFormInput,
    name: String,
    replacement: JetWebFormFieldSpec,
) -> Result<JetWebFormInput, String> {
    input.replace(name, replacement)
}

pub fn jet_web_forms_typed_set(
    form: &JetWebFormTyped,
    name: String,
    value: String,
) -> Result<(), String> {
    form.set(name, value)
}

pub fn jet_web_forms_typed_set_async_validator<F>(
    form: &JetWebFormTyped,
    name: String,
    timing: JetWebFormValidationTiming,
    debounce_ms: u64,
    validator: F,
) -> Result<(), String>
where
    F: Fn(String) -> Result<(), String> + Send + Sync + 'static,
{
    form.set_async_validator(name, timing, debounce_ms, validator)
}

pub fn jet_web_forms_typed_blur(
    form: &JetWebFormTyped,
    name: String,
) -> Result<(), String> {
    form.blur(name)
}

pub fn jet_web_forms_typed_validate(form: &JetWebFormTyped) -> Result<(), String> {
    form.validate()
}

pub fn jet_web_forms_typed_validate_field<F>(
    form: &JetWebFormTyped,
    name: String,
    timing: JetWebFormValidationTiming,
    validator: F,
) -> JetWebFormValidationChain
where
    F: Fn(String) -> Result<(), String> + Send + Sync + 'static,
{
    JetWebFormValidationChain::new(form.clone()).validate(name, timing, validator)
}

pub fn jet_web_forms_typed_validation_render(
    chain: &JetWebFormValidationChain,
) -> String {
    chain.render()
}

pub fn jet_web_forms_typed_validate_async(
    form: &JetWebFormTyped,
) -> JetWebFormTypedValidation {
    form.validate_async()
}

pub fn jet_web_forms_typed_submit_async(
    form: &JetWebFormTyped,
) -> JetWebFormTypedSubmission {
    form.submit_async()
}

pub fn jet_web_forms_typed_post(
    form: &JetWebFormTyped,
    body: String,
) -> Result<String, String> {
    form.post(body)
}

pub fn jet_web_forms_typed_decode_post(
    form: &JetWebFormTyped,
    body: String,
) -> Result<JetWebFormDecodedInput, String> {
    form.decode_post(&body)
}

pub fn jet_web_forms_typed_select_field(
    form: &JetWebFormTyped,
    name: String,
) -> Result<jet_std::JetDerived<JetWebFormFieldState>, String> {
    form.select_field(name)
}

pub fn jet_web_forms_typed_set_action<F>(
    form: &JetWebFormTyped,
    handler: F,
)
where
    F: Fn(JetWebFormDecodedInput) -> Result<String, JetWebFormActionError>
        + Send
        + Sync
        + 'static,
{
    form.set_action(handler);
}
pub fn jet_web_forms_typed_validation_wait(
    validation: JetWebFormTypedValidation,
) -> Result<(), String> {
    validation.wait()
}

pub fn jet_web_forms_typed_validation_cancel(
    validation: &JetWebFormTypedValidation,
) {
    validation.cancel();
}

pub fn jet_web_forms_typed_submission_wait(
    submission: JetWebFormTypedSubmission,
) -> Result<String, String> {
    submission.wait()
}

pub fn jet_web_forms_typed_submission_cancel(
    submission: &JetWebFormTypedSubmission,
) {
    submission.cancel();
}
pub fn jet_web_forms_action_error() -> JetWebFormActionError {
    JetWebFormActionError::default()
}

pub fn jet_web_forms_action_field_error(
    error: JetWebFormActionError,
    name: String,
    message: String,
) -> JetWebFormActionError {
    error.field(name, message)
}

pub fn jet_web_forms_action_form_error(
    error: JetWebFormActionError,
    message: String,
) -> JetWebFormActionError {
    error.form(message)
}
