//! Recipe MCP tool — `use_recipe`.
//!
//! A recipe is a composed screen the kit already ships. The rules name the
//! recipes and say to start from one, but a name is not an action: without a
//! tool that turns "take recipe X" into nodes, the model reads the rule as
//! advice and composes the screen itself. This tool is that action — the
//! recipe's master, instantiated onto the active page, ready to adapt.

use std::collections::BTreeMap;

use op_editor_core::NodeId;

use super::{EditorCommand, McpTool, ToolErrorCode, ToolOutcome};

/// First-party `use_recipe` tool.
pub struct UseRecipe;

impl McpTool for UseRecipe {
    fn name(&self) -> &str {
        "use_recipe"
    }

    fn call(&self, args: &BTreeMap<String, String>) -> ToolOutcome {
        let Some(recipe_id) = args.get("recipe_id").filter(|id| !id.is_empty()) else {
            return ToolOutcome::Err(
                ToolErrorCode::MissingArgument,
                "recipe_id is required".into(),
            );
        };
        let kit = op_editor_core::session_kit();
        let Some(recipe) = kit.recipes.iter().find(|recipe| &recipe.id == recipe_id) else {
            return ToolOutcome::Err(
                ToolErrorCode::InvalidArgument,
                format!("unknown recipe_id `{recipe_id}` — call list_recipes first"),
            );
        };
        let (doc_x, doc_y) = match (args.get("x"), args.get("y")) {
            (None, None) => (None, None),
            (x, y) => {
                // The closure answers with the MESSAGE, not with a
                // `ToolOutcome`: a `ToolOutcome` is the largest thing in this
                // crate's wire layer (it carries an `EditorCommand`), and an
                // error type that big inside `Result` makes every parse result
                // pay for a variant that is not there. The variant is built
                // once, at the return, where the code is known.
                let parse = |value: Option<&String>, key: &str| match value {
                    None => Ok(None),
                    Some(raw) if raw.is_empty() => Ok(None),
                    Some(raw) => raw
                        .parse::<f64>()
                        .map(Some)
                        .map_err(|_| format!("{key} must be a number")),
                };
                match (parse(x, "x"), parse(y, "y")) {
                    (Ok(x), Ok(y)) => (x, y),
                    (Err(message), _) | (_, Err(message)) => {
                        return ToolOutcome::Err(ToolErrorCode::InvalidArgument, message)
                    }
                }
            }
        };
        let mut out = BTreeMap::new();
        out.insert("recipe_id".into(), recipe.id.clone());
        out.insert("master_id".into(), recipe.template.clone());
        out.insert("wrote".into(), "true".into());
        // The merged session kit registers its masters as document components,
        // so a recipe is placed exactly like any other component — through
        // `InstantiateComponent`, by the master id the recipe names.
        let _ = (doc_x, doc_y, &kit);
        ToolOutcome::OkWithCommand(
            out,
            EditorCommand::InstantiateComponent {
                component_id: NodeId::new(recipe.template.clone()),
            },
        )
    }
}

pub fn use_recipe_snapshot() -> UseRecipe {
    UseRecipe
}

/// First-party `list_recipes` tool — the ids `use_recipe` accepts, so a model
/// never has to guess a name it cannot verify.
pub struct ListRecipes;

impl McpTool for ListRecipes {
    fn name(&self) -> &str {
        "list_recipes"
    }

    fn call(&self, _args: &BTreeMap<String, String>) -> ToolOutcome {
        let kit = op_editor_core::session_kit();
        let mut out = BTreeMap::new();
        out.insert(
            "recipes".into(),
            kit.recipes
                .iter()
                .map(|recipe| format!("{}|{}|{}", recipe.id, recipe.template, recipe.name))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        out.insert("count".into(), kit.recipes.len().to_string());
        ToolOutcome::Ok(out)
    }
}

pub fn list_recipes_snapshot() -> ListRecipes {
    ListRecipes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn a_known_recipe_instantiates_its_master() {
        let kit = op_editor_core::session_kit();
        let recipe = kit.recipes.first().expect("the kit ships recipes");
        match UseRecipe.call(&args(&[("recipe_id", &recipe.id)])) {
            ToolOutcome::OkWithCommand(
                out,
                EditorCommand::InstantiateComponent { component_id },
            ) => {
                assert_eq!(component_id, NodeId::new(recipe.template.clone()));
                assert_eq!(out.get("master_id"), Some(&recipe.template));
            }
            other => panic!("expected an instantiate command, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_recipe_is_rejected_with_its_own_name() {
        match UseRecipe.call(&args(&[("recipe_id", "nope")])) {
            ToolOutcome::Err(_, message) => assert!(message.contains("nope")),
            other => panic!("expected an error, got {other:?}"),
        }
        assert!(matches!(
            UseRecipe.call(&args(&[])),
            ToolOutcome::Err(ToolErrorCode::MissingArgument, _)
        ));
    }

    #[test]
    fn list_recipes_names_every_recipe_use_recipe_accepts() {
        let kit = op_editor_core::session_kit();
        match ListRecipes.call(&BTreeMap::new()) {
            ToolOutcome::Ok(out) => {
                assert_eq!(out.get("count"), Some(&kit.recipes.len().to_string()));
                for recipe in &kit.recipes {
                    assert!(out["recipes"].contains(&recipe.id));
                    assert!(out["recipes"].contains(&recipe.template));
                }
            }
            other => panic!("expected ok, got {other:?}"),
        }
    }
}
