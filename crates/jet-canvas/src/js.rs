/// Assemble the Canvas browser runtime from independently lintable JavaScript assets.
pub fn canvas_js() -> String {
    concat!(
        "(function () {\n",
        include_str!("js/runtime-state.js"),
        include_str!("js/editing-history.js"),
        include_str!("js/diagnostics-query.js"),
        include_str!("js/drawing-palette.js"),
        include_str!("js/project-navigation.js"),
        include_str!("js/graph-rendering.js"),
        include_str!("js/inspector-connections.js"),
        include_str!("js/input-events.js"),
        include_str!("js/transactions-catalog.js"),
        include_str!("js/bootstrap.js"),
        "})();\n",
    )
    .to_string()
}
