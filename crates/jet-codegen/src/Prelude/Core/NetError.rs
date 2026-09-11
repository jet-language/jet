// D-NET-ERROR-DISPLAY1: one NetError display projection for every tier.
//
// AOT's `JetShow`/`JetDisplay`, the resident host, and the TIR evaluator all
// marshal the checked surface ordinals into this kernel. Engines carry values;
// this source owns the user-facing failure text.

pub(crate) fn jet_net_error_kernel_show(
    variant: i64,
    detail_message: Option<&str>,
    dns_variant: Option<i64>,
    dns_value: Option<&str>,
) -> String {
    match variant {
        0..=13 => detail_message.unwrap_or_default().to_string(),
        14 => match (dns_variant, dns_value) {
            (Some(0), Some(name)) => format!("DNS name not found: `{name}`"),
            (Some(1), Some(message)) => message.to_string(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}
