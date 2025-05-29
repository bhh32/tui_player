use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent};

/// Event utility functions
pub mod event_utils {
    use super::*;
    
    /// Check if a key event matches Ctrl+C or Ctrl+Q (terminate)
    pub fn is_terminate_event(event: &Event) -> bool {
        matches!(
            event,
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }) | Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                modifiers: KeyModifiers::CONTROL,
                ..
            })
        )
    }
    
    /// Check if a key event is a navigation event
    pub fn is_navigation_event(event: &Event) -> bool {
        matches!(
            event,
            Event::Key(KeyEvent {
                code: KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right | KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End,
                ..
            }) | Event::Mouse(MouseEvent { .. })
        )
    }
}