use super::native_tool::{Tool, ToolChoice, ToolSpec, TOOL_NOOP};
use super::native_tool_adapter::AssessmentMode;
use super::repair::RepairContext;
use super::NativeConversationMessage;

#[derive(Clone, Debug)]
pub struct CallDispatch<'a> {
    pub assessment_mode: AssessmentMode,
    pub repair: Option<&'a RepairContext>,
    pub tools: Vec<ToolSpec>,
    pub tool_choice: ToolChoice,
    pub auto_tool_mode: bool,
    pub conversation: Vec<NativeConversationMessage>,
}

impl Default for CallDispatch<'_> {
    fn default() -> Self {
        Self {
            assessment_mode: AssessmentMode::NativeTools,
            repair: None,
            tools: Vec::new(),
            tool_choice: ToolChoice::Auto,
            auto_tool_mode: true,
            conversation: Vec::new(),
        }
    }
}

impl<'a> CallDispatch<'a> {
    pub fn tool_call(tool_specs: Vec<ToolSpec>) -> Self {
        Self {
            assessment_mode: AssessmentMode::NativeTools,
            repair: None,
            tools: tool_specs,
            tool_choice: ToolChoice::Auto,
            auto_tool_mode: false,
            conversation: Vec::new(),
        }
    }

    pub fn tool_probe() -> Self {
        Self {
            assessment_mode: AssessmentMode::NativeTools,
            repair: None,
            tools: vec![Tool::NoOp.spec(true)],
            tool_choice: ToolChoice::Specific(TOOL_NOOP),
            auto_tool_mode: false,
            conversation: Vec::new(),
        }
    }

    pub fn named_tool_probe(spec: ToolSpec, choice: ToolChoice) -> Self {
        Self {
            assessment_mode: AssessmentMode::NativeTools,
            repair: None,
            tools: vec![spec],
            tool_choice: choice,
            auto_tool_mode: false,
            conversation: Vec::new(),
        }
    }

    pub fn repair_tool_call(context: &'a RepairContext) -> Self {
        Self::repair_tool_call_with_tools(context, Vec::new())
    }

    pub fn repair_tool_call_with_tools(context: &'a RepairContext, tools: Vec<ToolSpec>) -> Self {
        Self {
            assessment_mode: AssessmentMode::NativeTools,
            repair: Some(context),
            tools,
            tool_choice: ToolChoice::Auto,
            auto_tool_mode: false,
            conversation: Vec::new(),
        }
    }

    pub fn with_conversation(mut self, conversation: Vec<NativeConversationMessage>) -> Self {
        self.conversation = conversation;
        self
    }
}
