//! The code style of the formatter, read in the settings: the options the JetBrains plugin shows on its
//! Code Style page (`LuaLanguageCodeStyleSettingsProvider`), with their IntelliJ defaults.

use crate::format::Style;
use crate::settings::Settings;

pub const SPACE_AFTER_FIELD_SEP: &str = "format.spaceAfterTableFieldSep";
pub const SPACE_AROUND_BINARY_OPERATOR: &str = "format.spaceAroundBinaryOperator";
pub const SPACE_INSIDE_INLINE_TABLE: &str = "format.spaceInsideInlineTable";
pub const SPACE_AROUND_ASSIGNMENT: &str = "format.spaceAroundAssignment";
pub const SPACE_BEFORE_COMMA: &str = "format.spaceBeforeComma";
pub const SPACE_AFTER_COMMA: &str = "format.spaceAfterComma";
pub const ALIGN_PARAMETERS: &str = "format.alignParameters";
pub const ALIGN_ARGUMENTS: &str = "format.alignArguments";
pub const KEEP_SIMPLE_BLOCKS: &str = "format.keepSimpleBlocksInOneLine";
pub const ALIGN_CONSECUTIVE_ASSIGNMENTS: &str = "format.alignConsecutiveAssignments";
pub const ALIGN_TABLE_FIELD_ASSIGN: &str = "format.alignTableFieldAssign";
pub const REMOVE_SEMICOLONS: &str = "format.removeSemicolons";
pub const CONTINUATION_INDENT: &str = "format.continuationIndent";

/// The style of an editor that indents by `tab_size` columns, with spaces or with tabs.
pub fn style(settings: &Settings, tab_size: u32, spaces: bool) -> Style {
    let default = Style::new(tab_size, spaces);
    // 0 is IntelliJ's ratio: a continuation is indented twice.
    let continuation = match settings.int(CONTINUATION_INDENT, 0) {
        columns @ 1..=64 => columns as u32,
        _ => default.continuation,
    };
    Style {
        continuation,
        space_after_field_sep: settings.bool(SPACE_AFTER_FIELD_SEP, default.space_after_field_sep),
        space_around_binary_operator: settings.bool(SPACE_AROUND_BINARY_OPERATOR, default.space_around_binary_operator),
        space_inside_inline_table: settings.bool(SPACE_INSIDE_INLINE_TABLE, default.space_inside_inline_table),
        space_around_assignment: settings.bool(SPACE_AROUND_ASSIGNMENT, default.space_around_assignment),
        space_before_comma: settings.bool(SPACE_BEFORE_COMMA, default.space_before_comma),
        space_after_comma: settings.bool(SPACE_AFTER_COMMA, default.space_after_comma),
        align_parameters: settings.bool(ALIGN_PARAMETERS, default.align_parameters),
        align_arguments: settings.bool(ALIGN_ARGUMENTS, default.align_arguments),
        keep_simple_blocks: settings.bool(KEEP_SIMPLE_BLOCKS, default.keep_simple_blocks),
        align_consecutive_assignments: settings
            .bool(ALIGN_CONSECUTIVE_ASSIGNMENTS, default.align_consecutive_assignments),
        align_table_field_assign: settings.bool(ALIGN_TABLE_FIELD_ASSIGN, default.align_table_field_assign),
        remove_semicolons: settings.bool(REMOVE_SEMICOLONS, default.remove_semicolons),
        ..default
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn the_options_are_declared_with_the_defaults_of_intellij() {
        let schema: Value = serde_json::from_str(include_str!("../../settings.schema.json")).expect("the schema");
        let default = Style::new(4, true);
        let declared = [
            (SPACE_AFTER_FIELD_SEP, default.space_after_field_sep),
            (SPACE_AROUND_BINARY_OPERATOR, default.space_around_binary_operator),
            (SPACE_INSIDE_INLINE_TABLE, default.space_inside_inline_table),
            (SPACE_AROUND_ASSIGNMENT, default.space_around_assignment),
            (SPACE_BEFORE_COMMA, default.space_before_comma),
            (SPACE_AFTER_COMMA, default.space_after_comma),
            (ALIGN_PARAMETERS, default.align_parameters),
            (ALIGN_ARGUMENTS, default.align_arguments),
            (KEEP_SIMPLE_BLOCKS, default.keep_simple_blocks),
            (ALIGN_CONSECUTIVE_ASSIGNMENTS, default.align_consecutive_assignments),
            (ALIGN_TABLE_FIELD_ASSIGN, default.align_table_field_assign),
            (REMOVE_SEMICOLONS, default.remove_semicolons),
        ];
        for (key, value) in declared {
            assert_eq!(schema["properties"][key]["default"], Value::Bool(value), "{key}");
        }
        assert_eq!(schema["properties"][CONTINUATION_INDENT]["default"], Value::from(0));
        let settings = Settings::defaults().with(KEEP_SIMPLE_BLOCKS, true.into()).with(SPACE_AFTER_COMMA, false.into());
        let read = style(&settings, 2, false);
        assert!(read.keep_simple_blocks && !read.space_after_comma && read.space_around_assignment);
        assert_eq!((read.indent, read.continuation, read.tabs), (2, 4, true), "twice the indent of the editor");
        let four = style(&settings.with(CONTINUATION_INDENT, 4.into()), 4, true);
        assert_eq!((four.indent, four.continuation), (4, 4));
    }
}
