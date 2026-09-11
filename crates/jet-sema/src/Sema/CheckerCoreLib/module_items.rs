use crate::Syntax;

/// Canonical Core module member names (completion, help, diagnostics).
pub fn core_module_items(module: &str) -> Vec<String> {
    if module == "core.compiler.lang" {
        return crate::Policy::RULE_ARG_DECLARATIONS
            .iter()
            .map(|declaration| declaration.name.to_string())
            .collect();
    }
    if module == Syntax::CORE_MEM_MODULE {
        return Syntax::CORE_MEM_GATE_TIERS
            .iter()
            .map(|(item, _)| (*item).to_string())
            .collect();
    }
    jet_foundation::CoreModuleExports::core_modules()
        .iter()
        .find(|declaration| declaration.module == module)
        .map_or(&[][..], |declaration| declaration.members)
        .iter()
        .map(|item| (*item).to_string())
        .collect()
}
pub fn core_module_type_item(module: &str, item: &str) -> bool {
    if jet_foundation::CoreModuleExports::core_leaf_kind(module, item).is_some() {
        return true;
    }
    // Remaining arms are sema-only nominal surfaces (for example vault
    // handles) that are not declared Core exports; exported types use the
    // descriptor lookup above.
    matches!(
        (module, item),
        ("core.crypto", "Secret" | "SigningKey" | "VerifyKey" | "X25519SecretKey"
            | "X25519PublicKey" | "SharedSecret" | "Signature" | "Sealed" | "WrappedKey"
            | "WrappedVaultKey" | "KeyUnlock" | "KeyWrapError" | "PasswordHash"
            | "Digest256" | "Digest512" | "Hasher" | "CryptoError" | "FileCryptoError")
        | ("core.build",
            "BuildGraph" | "BuildGraphTarget" | "BuildGraphAction" | "BuildGraphFile"
            | "BuildGraphNode" | "BuildGraphInputDigest" | "BuildGraphActionKey"
            | "BuildGraphFileDelta" | "BuildGraphKeyDelta" | "BuildGraphCacheDelta"
            | "BuildGraphDiff")
        | ("core.data",
            "DataAuthority" | "DataColumn" | "DataError" | "DataErrorKind"
            | "DataFormat" | "DataFreshness" | "DataInvalidationCause" | "DataLimits"
            | "DataLineOptions" | "DataLoader" | "DataLoaderKind" | "DataLoaderStatus"
            | "DataPivotCell" | "DataProvenance" | "Query" | "DataSchema" | "DataSnapshot"
            | "DataSnapshotIdentity" | "DataSourceIdentity" | "DataStatus" | "DataStream"
            | "Group"
            | "JetDataPlotAccessibility" | "JetDataPlotAggregate" | "JetDataPlotAxis"
            | "JetDataPlotBackend" | "JetDataPlotCapability" | "JetDataPlotChannel"
            | "JetDataPlotColumn" | "JetDataPlotDomain" | "JetDataPlotEncoding"
            | "JetDataPlotError" | "JetDataPlotErrorKind" | "JetDataPlotFacet"
            | "JetDataPlotFacetKind" | "JetDataPlotField" | "JetDataPlotFilterOp"
            | "JetDataPlotInspection" | "JetDataPlotInteraction" | "JetDataPlotLayer"
            | "JetDataPlotLayout" | "JetDataPlotLegend" | "JetDataPlotLegendPosition"
            | "JetDataPlotMark" | "JetDataPlotPlan" | "JetDataPlotProjection"
            | "JetDataPlotRender" | "JetDataPlotRenderFormat" | "JetDataPlotScale"
            | "JetDataPlotScaleKind" | "JetDataPlotSchema" | "JetDataPlotSelectedRow"
            | "JetDataPlotSourceFacts" | "JetDataPlotSupport" | "JetDataPlotTransform"
            | "JetDataPlotValue")
        | ("core.data.plot",
            "JetDataPlotAccessibility" | "JetDataPlotAggregate" | "JetDataPlotAxis"
            | "JetDataPlotBackend" | "JetDataPlotCapability" | "JetDataPlotChannel"
            | "JetDataPlotColumn" | "JetDataPlotDomain" | "JetDataPlotEncoding"
            | "JetDataPlotError" | "JetDataPlotErrorKind" | "JetDataPlotFacet"
            | "JetDataPlotFacetKind" | "JetDataPlotField" | "JetDataPlotFilterOp"
            | "JetDataPlotInspection" | "JetDataPlotInteraction" | "JetDataPlotLayer"
            | "JetDataPlotLayout" | "JetDataPlotLegend" | "JetDataPlotLegendPosition"
            | "JetDataPlotMark" | "JetDataPlotPlan" | "JetDataPlotProjection"
            | "JetDataPlotRender" | "JetDataPlotRenderFormat" | "JetDataPlotScale"
            | "JetDataPlotScaleKind" | "JetDataPlotSchema" | "JetDataPlotSelectedRow"
            | "JetDataPlotSourceFacts" | "JetDataPlotSupport" | "JetDataPlotTransform"
            | "JetDataPlotValue")
        | ("core.email",
            "Address" | "Message" | "Attachment" | "Envelope" | "SMTPSecurity"
            | "RecipientPolicy" | "RecipientReport" | "SendReport" | "EmailError"
            | "Limits" | "SMTPAuth" | "TLSTrust" | "DkimConfig" | "SMTPConfig"
            | "Mailer")
        | ("core.encoding",
            "DataTree" | "EncodingFormat" | "EncodingLimits" | "EncodingError" | "EncodingCause"
            | "EncodingErrorKind" | "DataEvent")
        | ("core.encoding.cbor",
            "CBORReader" | "CBORWriter" | "CBOROptions" | "CBORError" | "CBORErrorKind")
        | ("core.encoding.csv", "CSVReader" | "CSVWriter" | "CSVRow")
        | ("core.encoding.json", "JSONReader" | "JSONWriter")
        | ("core.encoding.jsonl", "JSONLReader" | "JSONLWriter")
        | ("core.encoding.xml", "XMLReader" | "XMLWriter")
        | ("core.files", "FileScope")
        | ("core.mem", "AllocError" | "Atomic")
        | ("core.sys", "EnvError")
        // D-FOUND-LIFECYCLE1=A: typed process-signal lifecycle control.
        // D-FOUND-REALTIME1=A: callback streams expose one typed lifecycle pair.
        | ("core.rt", "RealtimeStream" | "RealtimeReceipt")
        | ("core.process", "ProcessSignal")
        // D-FOUND-PLATFORM1=A: one typed host/font value vocabulary.
        | ("core.font", "FontFace" | "FontStyle" | "Glyph" | "GlyphRun" | "GlyphShaper")
        | ("core.ui",
            "UiImeMode" | "UiPlayground" | "UiPreview" | "UiPreviewAccessibility"
            | "UiPreviewAuthority" | "UiPreviewContext" | "UiPreviewDevice"
            | "UiPreviewEffect" | "UiPreviewInputOverride" | "UiPreviewInputValue"
            | "UiPreviewKind" | "UiPreviewLifecycle" | "UiPreviewRegistry"
            | "UiPreviewSource" | "UiPreviewTheme" | "UiPreviewTraits" | "UiPreviewViewport")
        | ("core.tui",
            "TuiEvent" | "TuiColorProfile" | "TuiColor" | "TuiCapabilities"
            | "TuiStyle" | "TuiConstraint" | "TuiDirection" | "TuiListState")
        | ("core.ui.host",
            "UiCapability" | "UiCapabilityFact" | "UiCapabilityFacts" | "UiCancellation"
            | "UiHostError" | "UiServiceResult" | "UiFileDialogKind" | "UiFsAccess"
            | "UiFsRights" | "UiFsGrant" | "UiGrantedPath" | "UiFileFilter"
            | "UiFileDialogRequest" | "UiFileDialogSelection" | "UiClipboardText"
            | "UiClipboardWrite" | "UiTextRange" | "UiImeMode" | "UiImePhase"
            | "UiImeComposition" | "UiImeEvent" | "UiDragOperation" | "UiDropItem"
            | "UiDragPhase" | "UiDragEvent" | "UiShortcutModifier" | "UiShortcutModifiers"
            | "UiShortcut" | "UiShortcutBinding" | "UiShortcutDispatch"
            | "UiAccessibilityState" | "UiAccessibility" | "UiNodeId"
            | "UiAccessibilityProjection" | "UiFileFilterResult" | "UiFsGrantResult"
            | "UiShortcutResult" | "UiShortcutBindingResult" | "UiAccessibilityResult"
            | "UiFileDialogResult" | "UiClipboardTextResult" | "UiClipboardWriteResult"
            | "UiImeResult" | "UiDragResult" | "UiShortcutDispatchResult"
            | "UiAccessibilityNodeResult" | "UiAccessibilityAttachResult"
            | "UiAccessibilityProjectionResult")
        | ("core.web.forms",
            "WebFormValueType" | "WebFormStatus" | "WebFormFieldState"
            | "WebFormState" | "WebForm" | "WebFormValidation"
            | "WebFormControl" | "WebFormValidationTiming" | "WebFormFieldSpec"
            | "WebFormInput" | "WebFormDecodedInput" | "WebFormActionError"
            | "WebFormTyped" | "WebFormValidationChain"
            | "WebFormTypedValidation" | "WebFormTypedSubmission")
        // D-BROWSER-AUTO1=A: native BiDi and browser-test values.
        | ("core.web.browser",
            "Browser" | "BrowserContext" | "BrowserPage" | "BrowserFrame" | "BrowserLocator"
            | "BrowserIntercept"
            | "BrowserEvent" | "BrowserTrace" | "BrowserReceipt" | "BrowserPrivacy" | "BrowserError"
            | "BrowserAbilities"
            | "BrowserProfile" | "BrowserTimeout" | "BrowserProtocol" | "BrowserLocked"
            | "BrowserTestConfig" | "BrowserTestSource" | "BrowserTestAction"
            | "BrowserTestSnapshot" | "BrowserTestEventFact" | "BrowserTestArtifact"
            | "BrowserTestAttempt" | "BrowserTestCase" | "BrowserTestReport"
            | "BrowserTestFixture" | "BrowserTestServer")
        // D-FLAGSHIP-WEBAPI1=A: headless web suite modules own their
        // nominal values; none are re-exported from root `core.web`.
        | ("core.web.router",
            "WebRouterValueType" | "WebRouterField" | "WebRouterSearchCodec"
            | "WebRouterCacheStatus" | "WebRouterCacheState" | "WebNavigationStatus"
            | "WebNavigation" | "WebNavigationState")
        | ("core.web.query", "WebMutationState" | "WebMutationStatus" | "WebQueryStatus" | "WebQueryNetworkMode" | "WebQueryState" | "WebQuery")
        | ("core.web.table",
            "WebTableSortDirection" | "WebTablePageMode" | "WebTableSort" | "WebTableFilter"
            | "WebTableState" | "WebTableColumn" | "WebTablePage" | "WebTableRow" | "WebTable")
        | ("core.web.virtual",
            "WebVirtualWindow" | "WebVirtualPlan" | "WebVirtualPlanViewport")
        | ("core.web.store",
            "WebStoreTransaction" | "WebStoreEvent" | "WebStoreInspection"
            | "WebStorePatch" | "WebStore" | "WebStoreSubscription")
        // D-WEBAPP1=D: namespaced `web.App` / `web.Page` / `web.Context` / `web.Mount`.
        | ("core.web", "App" | "Page" | "Context" | "Mount")
    )
}
