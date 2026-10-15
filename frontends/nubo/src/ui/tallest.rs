//! A widget that shows one element at the height of the tallest of
//! several, such as the open page of the settings window at the height
//! of its tallest page, so the window keeps its size from tab to tab.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{Clipboard, Shell, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

/// Shows the first of its elements, as tall as the tallest of all of them
/// would be with the width it gets, and no taller than the space it has.
/// The others are only measured: they are never drawn and get no events.
pub struct Tallest<'a, Message, Theme, Renderer> {
    /// The element shown, then the ones only measured.
    elements: Vec<Element<'a, Message, Theme, Renderer>>,
}

/// Shows `shown` as tall as the tallest of it and `measured` (see
/// [`Tallest`]). `shown` should fill the height it gets, such as a
/// scrollable whose height is `Fill`, and each of `measured` should shrink
/// to its content.
pub fn tallest<'a, Message, Theme, Renderer>(
    shown: impl Into<Element<'a, Message, Theme, Renderer>>,
    measured: impl IntoIterator<Item = Element<'a, Message, Theme, Renderer>>,
) -> Tallest<'a, Message, Theme, Renderer> {
    let mut elements = vec![shown.into()];
    elements.extend(measured);
    Tallest { elements }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Tallest<'_, Message, Theme, Renderer>
where
    Renderer: iced::advanced::Renderer,
{
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn children(&self) -> Vec<Tree> {
        self.elements.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.elements);
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let width = limits.max().width;
        let unbounded = layout::Limits::new(Size::ZERO, Size::new(width, f32::INFINITY));
        let tallest = self.elements[1..]
            .iter_mut()
            .zip(&mut tree.children[1..])
            .map(|(element, tree)| {
                element
                    .as_widget_mut()
                    .layout(tree, renderer, &unbounded)
                    .size()
                    .height
            })
            .fold(0.0, f32::max);

        let height = tallest.min(limits.max().height);
        let exact = layout::Limits::new(Size::new(0.0, height), Size::new(width, height));
        let shown =
            self.elements[0]
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, &exact);
        let size = limits.resolve(Length::Fill, Length::Shrink, Size::new(width, height));
        layout::Node::with_children(size, vec![shown])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.elements[0].as_widget_mut().operate(
            &mut tree.children[0],
            shown_layout(layout),
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.elements[0].as_widget_mut().update(
            &mut tree.children[0],
            event,
            shown_layout(layout),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.elements[0].as_widget().mouse_interaction(
            &tree.children[0],
            shown_layout(layout),
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.elements[0].as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            shown_layout(layout),
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.elements[0].as_widget_mut().overlay(
            &mut tree.children[0],
            shown_layout(layout),
            renderer,
            viewport,
            translation,
        )
    }
}

/// The layout of the element shown, the only child [`Tallest::layout`]
/// keeps.
fn shown_layout(layout: Layout<'_>) -> Layout<'_> {
    layout
        .children()
        .next()
        .expect("the layout keeps the shown element")
}

impl<'a, Message, Theme, Renderer> From<Tallest<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(tallest: Tallest<'a, Message, Theme, Renderer>) -> Self {
        Element::new(tallest)
    }
}
