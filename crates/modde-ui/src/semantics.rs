use iced::Element;
use iced::widget::{Id, container};

use crate::app::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    Button,
    Checkbox,
    TextInput,
    PickList,
    Scrollable,
    Text,
    Dialog,
    Tab,
    List,
    ListItem,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub role: Role,
    pub label: Option<String>,
    pub test_id: Option<String>,
}

impl Metadata {
    pub fn new(role: Role) -> Self {
        Self {
            role,
            label: None,
            test_id: None,
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn test_id(mut self, test_id: impl Into<String>) -> Self {
        self.test_id = Some(test_id.into());
        self
    }
}

pub fn widget_id(test_id: impl Into<String>) -> Id {
    format!("test-id:{}", test_id.into()).into()
}

pub fn test_id<'a>(
    test_id: impl Into<String>,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    container(content).id(widget_id(test_id)).into()
}
