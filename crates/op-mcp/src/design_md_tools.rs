//! TS-compatible `design.md` MCP tools.

use std::collections::BTreeMap;

use op_editor_core::{
    effective_design_rules, extract_design_md_from_document, generate_design_md, parse_design_md,
    DesignRuleKind, DesignRuleScope, DesignRuleSource, EditorCommand, EditorState,
    EffectiveDesignRule,
};

use super::{McpTool, ToolErrorCode, ToolOutcome};

pub struct GetDesignMd {
    spec: Option<op_editor_core::DesignMdSpec>,
    extracted: op_editor_core::DesignMdSpec,
}

pub struct SetDesignMd {
    extracted: op_editor_core::DesignMdSpec,
}

pub struct ExportDesignMd {
    spec: Option<op_editor_core::DesignMdSpec>,
    extracted: op_editor_core::DesignMdSpec,
}

pub struct ListDesignRules {
    rules: Vec<EffectiveDesignRule>,
}

pub struct GetDesignRule {
    rules: Vec<EffectiveDesignRule>,
}

pub struct GetEffectiveDesignRules {
    rules: Vec<EffectiveDesignRule>,
}

impl McpTool for GetDesignMd {
    fn name(&self) -> &str {
        "get_design_md"
    }

    fn call(&self, _args: &BTreeMap<String, String>) -> ToolOutcome {
        let mut out = BTreeMap::new();
        let selected = self
            .spec
            .as_ref()
            .or_else(|| spec_has_content(&self.extracted).then_some(&self.extracted));
        let Some(spec) = selected else {
            out.insert("hasDesignMd".into(), "false".into());
            return ToolOutcome::Ok(out);
        };
        match spec_json(spec) {
            Ok(json) => {
                out.insert("hasDesignMd".into(), "true".into());
                out.insert("spec".into(), json);
                out.insert("markdown".into(), generate_design_md(spec));
                ToolOutcome::Ok(out)
            }
            Err(e) => ToolOutcome::Err(ToolErrorCode::Internal, e.to_string()),
        }
    }
}

impl McpTool for SetDesignMd {
    fn name(&self) -> &str {
        "set_design_md"
    }

    fn call(&self, args: &BTreeMap<String, String>) -> ToolOutcome {
        let auto_extract = match parse_bool_arg(args, "autoExtract") {
            Ok(v) => v,
            Err(e) => return ToolOutcome::Err(ToolErrorCode::InvalidArgument, e.to_string()),
        };
        let spec = if auto_extract {
            self.extracted.clone()
        } else if let Some(markdown) = args.get("markdown").filter(|s| !s.is_empty()) {
            parse_design_md(markdown)
        } else {
            let mut out = BTreeMap::new();
            out.insert("success".into(), "false".into());
            return ToolOutcome::Ok(out);
        };
        let spec_json = match spec_json(&spec) {
            Ok(json) => json,
            Err(e) => return ToolOutcome::Err(ToolErrorCode::Internal, e.to_string()),
        };
        let mut out = BTreeMap::new();
        out.insert("success".into(), "true".into());
        out.insert("wrote".into(), "true".into());
        out.insert("spec".into(), spec_json);
        ToolOutcome::OkWithCommand(
            out,
            EditorCommand::SetDesignMd {
                spec: Box::new(spec),
            },
        )
    }
}

impl McpTool for ExportDesignMd {
    fn name(&self) -> &str {
        "export_design_md"
    }

    fn call(&self, _args: &BTreeMap<String, String>) -> ToolOutcome {
        let spec = self.spec.as_ref().unwrap_or(&self.extracted);
        let mut out = BTreeMap::new();
        out.insert("markdown".into(), generate_design_md(spec));
        ToolOutcome::Ok(out)
    }
}

impl McpTool for ListDesignRules {
    fn name(&self) -> &str {
        "list_design_rules"
    }

    fn call(&self, args: &BTreeMap<String, String>) -> ToolOutcome {
        let rules = self
            .rules
            .iter()
            .filter(|entry| rule_matches(entry, args))
            .map(rule_json)
            .collect::<Vec<_>>();
        ToolOutcome::OkJson(serde_json::json!({ "count": rules.len(), "rules": rules }).to_string())
    }
}

impl McpTool for GetDesignRule {
    fn name(&self) -> &str {
        "get_design_rule"
    }

    fn call(&self, args: &BTreeMap<String, String>) -> ToolOutcome {
        let Some(id) = args.get("id").filter(|id| !id.is_empty()) else {
            return ToolOutcome::Err(ToolErrorCode::MissingArgument, "id is required".into());
        };
        let Some(rule) = self.rules.iter().find(|entry| entry.rule.id == *id) else {
            return ToolOutcome::Err(ToolErrorCode::ToolFailed, format!("rule not found: {id}"));
        };
        ToolOutcome::OkJson(serde_json::json!({ "rule": rule_json(rule) }).to_string())
    }
}

impl McpTool for GetEffectiveDesignRules {
    fn name(&self) -> &str {
        "get_effective_design_rules"
    }

    fn call(&self, _args: &BTreeMap<String, String>) -> ToolOutcome {
        let rules = self.rules.iter().map(rule_json).collect::<Vec<_>>();
        ToolOutcome::OkJson(
            serde_json::json!({
                "count": rules.len(),
                "rules": rules,
                "ordering": "priorityDescThenId",
            })
            .to_string(),
        )
    }
}

pub fn get_design_md_snapshot(state: &EditorState) -> GetDesignMd {
    GetDesignMd {
        spec: state.doc.design_md.clone(),
        extracted: extract_design_md_from_document(&state.doc),
    }
}

pub fn set_design_md_snapshot(state: &EditorState) -> SetDesignMd {
    SetDesignMd {
        extracted: extract_design_md_from_document(&state.doc),
    }
}

pub fn export_design_md_snapshot(state: &EditorState) -> ExportDesignMd {
    ExportDesignMd {
        spec: state.doc.design_md.clone(),
        extracted: extract_design_md_from_document(&state.doc),
    }
}

pub fn list_design_rules_snapshot(state: &EditorState) -> ListDesignRules {
    ListDesignRules {
        rules: effective_design_rules(state.doc.design_md.as_ref()),
    }
}

pub fn get_design_rule_snapshot(state: &EditorState) -> GetDesignRule {
    GetDesignRule {
        rules: effective_design_rules(state.doc.design_md.as_ref()),
    }
}

pub fn get_effective_design_rules_snapshot(state: &EditorState) -> GetEffectiveDesignRules {
    GetEffectiveDesignRules {
        rules: effective_design_rules(state.doc.design_md.as_ref()),
    }
}

fn rule_matches(entry: &EffectiveDesignRule, args: &BTreeMap<String, String>) -> bool {
    if let Some(kind) = args.get("kind").filter(|value| !value.is_empty()) {
        let actual = match entry.rule.kind {
            DesignRuleKind::Do => "do",
            DesignRuleKind::Dont => "dont",
            DesignRuleKind::Require => "require",
            DesignRuleKind::Avoid => "avoid",
        };
        if actual != kind.to_lowercase() {
            return false;
        }
    }
    if let Some(type_id) = args.get("typeId").filter(|value| !value.is_empty()) {
        if !matches!(
            &entry.rule.scope,
            DesignRuleScope::ComponentType { type_id: actual, .. } if actual == type_id
        ) {
            return false;
        }
    }
    if let Some(component_id) = args.get("componentId").filter(|value| !value.is_empty()) {
        if !matches!(
            &entry.rule.scope,
            DesignRuleScope::ComponentMaster { component_id: actual } if actual == component_id
        ) {
            return false;
        }
    }
    true
}

fn rule_json(entry: &EffectiveDesignRule) -> serde_json::Value {
    let source = match &entry.source {
        DesignRuleSource::Library { kit_id } => {
            serde_json::json!({ "type": "library", "kitId": kit_id })
        }
        DesignRuleSource::Document => serde_json::json!({ "type": "document" }),
        DesignRuleSource::DocumentOverride { overridden_rule_id } => serde_json::json!({
            "type": "documentOverride",
            "overriddenRuleId": overridden_rule_id,
        }),
    };
    serde_json::json!({ "value": entry.rule, "source": source })
}

fn spec_has_content(spec: &op_editor_core::DesignMdSpec) -> bool {
    spec.color_palette.as_ref().is_some_and(|c| !c.is_empty())
        || spec
            .typography
            .as_ref()
            .and_then(|t| t.font_family.as_ref())
            .is_some_and(|s| !s.is_empty())
        || spec.visual_theme.as_ref().is_some_and(|s| !s.is_empty())
}

/// A `design.md` tool failure. `SerializeSpec` is an internal fault
/// (`ToolErrorCode::Internal`); `BoolArg` is a caller fault
/// (`ToolErrorCode::InvalidArgument`) — the split the `String` version
/// could only express by which call site produced it. `Display`
/// reproduces the previous message byte-for-byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesignMdError {
    /// The spec did not serialize to JSON.
    SerializeSpec(String),
    /// A boolean arg is neither `"true"` nor `"false"`.
    BoolArg { key: String, raw: String },
}

impl std::fmt::Display for DesignMdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DesignMdError::SerializeSpec(detail) => {
                write!(f, "serialize design.md spec failed: {detail}")
            }
            DesignMdError::BoolArg { key, raw } => {
                write!(f, "{key} must be true or false, got {raw:?}")
            }
        }
    }
}

impl std::error::Error for DesignMdError {}

fn spec_json(spec: &op_editor_core::DesignMdSpec) -> Result<String, DesignMdError> {
    serde_json::to_string(spec).map_err(|e| DesignMdError::SerializeSpec(e.to_string()))
}

fn parse_bool_arg(args: &BTreeMap<String, String>, key: &str) -> Result<bool, DesignMdError> {
    let Some(raw) = args.get(key).or_else(|| args.get("auto_extract")) else {
        return Ok(false);
    };
    match raw.as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(DesignMdError::BoolArg {
            key: key.to_string(),
            raw: raw.clone(),
        }),
    }
}
