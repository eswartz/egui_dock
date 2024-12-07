use egui::{
    emath::TSTransform, epaint::TextShape, lerp, pos2, vec2, Align, Align2, Button, Color32,
    CursorIcon, Frame, Id, Key, LayerId, Layout, NumExt, Order, Rect, Response, Rounding,
    ScrollArea, Sense, Shape, Stroke, TextStyle, Ui, UiBuilder, Vec2, WidgetText,
};
use std::ops::RangeInclusive;

use crate::dock_area::tab_removal::TabRemoval;
use crate::{
    dock_area::{
        drag_and_drop::{DragData, DragDropState, HoverData, TreeComponent},
        state::State,
    },
    utils::{fade_visuals, rect_set_size_centered, rect_stroke_box},
    DockArea, Node, NodeIndex, Style, SurfaceIndex, TabAddAlign, TabIndex, TabStyle, TabViewer,
};

use crate::popup::popup_under_widget;

impl<Tab> DockArea<'_, Tab> {
    pub(super) fn show_leaf(
        &mut self,
        ui: &mut Ui,
        state: &mut State,
        (surface_index, node_index): (SurfaceIndex, NodeIndex),
        tab_viewer: &mut impl TabViewer<Tab = Tab>,
        fade_style: Option<(&Style, f32)>,
    ) {
        assert!(self.dock_state[surface_index][node_index].is_leaf());
        let collapsed = self.dock_state[surface_index][node_index].is_collapsed();

        let rect = self.dock_state[surface_index][node_index]
            .rect()
            .expect("This node must be a leaf");
        let ui = &mut ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::top_down_justified(Align::Min))
                .id_salt((node_index, "node")),
        );
        let spacing = ui.spacing().item_spacing;
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        ui.set_clip_rect(ui.clip_rect().intersect(rect));

        if self.dock_state[surface_index][node_index].tabs_count() == 0 {
            return;
        }
        let tabbar_rect = self.tab_bar(
            ui,
            state,
            (surface_index, node_index),
            tab_viewer,
            fade_style.map(|(style, _)| style),
            collapsed,
        );
        self.tab_body(
            ui,
            state,
            (surface_index, node_index),
            tab_viewer,
            spacing,
            tabbar_rect,
            fade_style,
            collapsed,
        );

        let tabs = self.dock_state[surface_index][node_index]
            .tabs_mut()
            .expect("This node must be a leaf here");
        for (tab_index, tab) in tabs.iter_mut().enumerate() {
            if tab_viewer.force_close(tab) {
                self.to_remove
                    .push((surface_index, node_index, TabIndex(tab_index)).into());
            }
        }
    }

    fn tab_bar(
        &mut self,
        ui: &mut Ui,
        state: &mut State,
        (surface_index, node_index): (SurfaceIndex, NodeIndex),
        tab_viewer: &mut impl TabViewer<Tab = Tab>,
        fade_style: Option<&Style>,
        collapsed: bool,
    ) -> Rect {
        assert!(self.dock_state[surface_index][node_index].is_leaf());

        let style = fade_style.unwrap_or_else(|| self.style.as_ref().unwrap());
        let (tabbar_outer_rect, tabbar_response) = ui.allocate_exact_size(
            vec2(ui.available_width(), style.tab_bar.height),
            Sense::hover(),
        );
        ui.painter().rect_filled(
            tabbar_outer_rect,
            style.tab_bar.rounding,
            style.tab_bar.bg_fill,
        );

        let mut available_width = tabbar_outer_rect.width();
        let scroll_bar_width = available_width;
        if available_width == 0.0 {
            return tabbar_outer_rect;
        }

        // Reserve space for the buttons at the ends of the tab bar.

        if self.show_add_buttons {
            available_width -= Style::TAB_ADD_BUTTON_SIZE;
        }

        if self.show_leaf_close_all_buttons {
            available_width -= Style::TAB_CLOSE_ALL_BUTTON_SIZE;
        }

        if self.show_leaf_collapse_buttons {
            available_width -= Style::TAB_COLLAPSE_BUTTON_SIZE;
        }

        let mut si_ni_ti = self.dock_state.scroll_to_tab.take();
        let mut tab_index_pos;

        let actual_width = {
            let Node::Leaf { tabs, scroll, .. } = &mut self.dock_state[surface_index][node_index]
            else {
                unreachable!()
            };

            let tabbar_inner_rect = Rect::from_min_size(
                (tabbar_outer_rect.min - pos2(-*scroll, 0.0)
                    + vec2(
                        if self.show_leaf_collapse_buttons {
                            Style::TAB_COLLAPSE_BUTTON_SIZE
                        } else {
                            0.0
                        },
                        0.0,
                    ))
                .to_pos2(),
                vec2(tabbar_outer_rect.width(), tabbar_outer_rect.height()),
            );

            let tabs_ui = &mut ui.new_child(
                UiBuilder::new()
                    .max_rect(tabbar_inner_rect)
                    .layout(Layout::left_to_right(Align::Center))
                    .id_salt("tabs"),
            );

            let mut clip_rect = tabbar_outer_rect;
            clip_rect.set_width(available_width);
            if self.show_leaf_collapse_buttons {
                clip_rect = clip_rect.translate(vec2(Style::TAB_COLLAPSE_BUTTON_SIZE, 0.0));
            }
            tabs_ui.set_clip_rect(tabs_ui.clip_rect().intersect(clip_rect));

            // Desired size for tabs in "expanded" mode.
            let prefered_width = style
                .tab_bar
                .fill_tab_bar
                .then_some(available_width / (tabs.len() as f32));

            tab_index_pos = self.tabs(
                tabs_ui,
                state,
                (surface_index, node_index),
                tab_viewer,
                tabbar_outer_rect,
                prefered_width,
                fade_style,
            );

            // Draw hline from tab end to edge of tab bar.
            let px = ui.ctx().pixels_per_point().recip();
            let style = fade_style.unwrap_or_else(|| self.style.as_ref().unwrap());

            ui.painter().hline(
                tabs_ui.min_rect().right().min(clip_rect.right())..=tabbar_outer_rect.right(),
                tabbar_outer_rect.bottom() - px,
                (px, style.tab_bar.hline_color),
            );

            // Add button at the ends of the tab bar.
            if self.show_add_buttons {
                let offset = match style.buttons.add_tab_align {
                    TabAddAlign::Left => {
                        (clip_rect.width() - tabs_ui.min_rect().width()).at_least(0.0)
                    }
                    TabAddAlign::Right => 0.0,
                } + if self.show_leaf_close_all_buttons {
                    Style::TAB_CLOSE_ALL_BUTTON_SIZE
                } else {
                    0.0
                };
                self.tab_plus(
                    ui,
                    surface_index,
                    node_index,
                    tab_viewer,
                    tabbar_outer_rect,
                    offset,
                    fade_style,
                );
            }

            if self.show_leaf_close_all_buttons {
                // Current leaf contains non-closable tabs.
                let disabled = if let Node::Leaf { tabs, .. } =
                    &mut self.dock_state[surface_index][node_index]
                {
                    !tabs.iter_mut().all(|tab| tab_viewer.closeable(tab))
                } else {
                    unreachable!()
                };

                // Current window contains non-closable tabs.
                let close_window_disabled = disabled
                    || !self.dock_state[surface_index].iter_mut().all(|node| {
                        if let Node::Leaf { tabs, .. } = node {
                            tabs.iter_mut().all(|tab| tab_viewer.closeable(tab))
                        } else {
                            true
                        }
                    });

                self.tab_close_all(
                    ui,
                    surface_index,
                    node_index,
                    tabbar_outer_rect,
                    fade_style,
                    disabled,
                    close_window_disabled,
                )
            }

            if self.show_leaf_collapse_buttons {
                self.tab_collapse(
                    ui,
                    surface_index,
                    node_index,
                    tabbar_outer_rect,
                    fade_style,
                    collapsed,
                )
            }

            tabs_ui.min_rect().width()
        };

        let scroll_to_tab = si_ni_ti.as_ref().is_some_and(|(si, ni, _)| tab_index_pos.is_some() && *si == surface_index && *ni == node_index);

        let mut scroll_target = if scroll_to_tab {
            Some(tab_index_pos.unwrap().1)
        } else {
            None
        };

        self.tab_bar_scroll(
            ui,
            state,
            (surface_index, node_index),
            actual_width,
            available_width,
            scroll_bar_width,
            &tabbar_response,
            fade_style,
            &mut scroll_target,
        );

        if scroll_to_tab {
            self.dock_state.scroll_to_tab = if scroll_target.is_none() { None } else { si_ni_ti };
        }

        tabbar_outer_rect
    }

    #[allow(clippy::too_many_arguments)]
    fn tabs(
        &mut self,
        tabs_ui: &mut Ui,
        state: &mut State,
        (surface_index, node_index): (SurfaceIndex, NodeIndex),
        tab_viewer: &mut impl TabViewer<Tab = Tab>,
        tabbar_outer_rect: Rect,
        preferred_width: Option<f32>,
        fade: Option<&Style>,
    ) -> Option<(TabIndex, f32)> {
        assert!(self.dock_state[surface_index][node_index].is_leaf());

        let focused = self.dock_state.focused_leaf();
        let tabs_len = {
            let tabs = self.dock_state[surface_index][node_index]
                .tabs()
                .expect("This node must be a leaf here");
            tabs.len()
        };

        let mut active_tab_index_pos = None;

        for tab_index in 0..tabs_len {
            let id = self
                .id
                .with((surface_index, "surface"))
                .with((node_index, "node"))
                .with((tab_index, "tab"));
            let tab_index = TabIndex(tab_index);
            let is_being_dragged = tabs_ui.ctx().is_being_dragged(id)
                && tabs_ui.input(|i| i.pointer.is_decidedly_dragging())
                && self.draggable_tabs;

            if is_being_dragged {
                tabs_ui.output_mut(|o| o.cursor_icon = CursorIcon::Grabbing);
            }

            let (is_active, label, tab_style, closeable) = {
                let Node::Leaf { tabs, active, .. } =
                    &mut self.dock_state[surface_index][node_index]
                else {
                    unreachable!()
                };
                let style = fade.unwrap_or_else(|| self.style.as_ref().unwrap());
                let tab_style = tab_viewer.tab_style_override(&tabs[tab_index.0], &style.tab);
                (
                    *active == tab_index || is_being_dragged,
                    tab_viewer.title(&mut tabs[tab_index.0]),
                    tab_style.unwrap_or(style.tab.clone()),
                    tab_viewer.closeable(&mut tabs[tab_index.0]),
                )
            };

            let show_close_button = self.show_close_buttons && closeable;

            let (response, title_id) = if is_being_dragged {
                let layer_id = LayerId::new(Order::Tooltip, id);
                let response = tabs_ui
                    .scope_builder(UiBuilder::new().layer_id(layer_id), |ui| {
                        self.tab_title(
                            ui,
                            &tab_style,
                            id,
                            label,
                            is_active && Some((surface_index, node_index)) == focused,
                            is_active,
                            is_being_dragged,
                            preferred_width,
                            show_close_button,
                            fade,
                        )
                    })
                    .response;
                let title_id = response.id;

                let response =
                    tabs_ui.interact(response.rect, id.with("dragged"), Sense::click_and_drag());

                if let Some(pointer_pos) = tabs_ui.ctx().pointer_interact_pos() {
                    let start = *state.drag_start.get_or_insert(pointer_pos);
                    let delta = pointer_pos - start;
                    if delta.x.abs() > 30.0 || delta.y.abs() > 6.0 {
                        tabs_ui
                            .ctx()
                            .transform_layer_shapes(layer_id, TSTransform::new(delta, 1.0));

                        tabs_ui.memory_mut(|mem| {
                            mem.data.insert_temp(
                                self.id.with("drag_data"),
                                Some(DragData {
                                    src: TreeComponent::Tab(surface_index, node_index, tab_index),
                                    rect: self.dock_state[surface_index][node_index]
                                        .rect()
                                        .unwrap(),
                                }),
                            );
                        });
                    }
                }

                (response, title_id)
            } else {
                let (mut response, close_response) = self.tab_title(
                    tabs_ui,
                    &tab_style,
                    id,
                    label,
                    is_active && Some((surface_index, node_index)) == focused,
                    is_active,
                    is_being_dragged,
                    preferred_width,
                    show_close_button,
                    fade,
                );
                let title_id = response.id;
                let close_clicked = close_response.is_some_and(|res| res.clicked());
                let is_lonely_tab = self.dock_state[surface_index].num_tabs() == 1;

                if self.show_tab_name_on_hover {
                    let tabs = self.dock_state[surface_index][node_index]
                        .tabs_mut()
                        .expect("This node must be a leaf");
                    let tab = &mut tabs[tab_index.0];
                    response = response.on_hover_ui(|ui| {
                        ui.label(tab_viewer.title(tab));
                    });
                }

                if self.tab_context_menus {
                    let eject_button =
                        Button::new(&self.dock_state.translations.tab_context_menu.eject_button);
                    let close_button =
                        Button::new(&self.dock_state.translations.tab_context_menu.close_button);

                    let Node::Leaf { tabs, active, .. } =
                        &mut self.dock_state[surface_index][node_index]
                    else {
                        unreachable!()
                    };
                    let tab = &mut tabs[tab_index.0];

                    response.context_menu(|ui| {
                        tab_viewer.context_menu(ui, tab, surface_index, node_index);
                        if (surface_index.is_main() || !is_lonely_tab)
                            && tab_viewer.allowed_in_windows(tab)
                            && ui.add(eject_button).clicked()
                        {
                            self.to_detach.push((surface_index, node_index, tab_index));
                            ui.close_menu();
                        }
                        if show_close_button && ui.add(close_button).clicked() {
                            if tab_viewer.on_close(tab) {
                                self.to_remove
                                    .push((surface_index, node_index, tab_index).into());
                            } else {
                                *active = tab_index;
                                self.new_focused = Some((surface_index, node_index));
                            }
                            ui.close_menu();
                        }
                    });
                }

                if close_clicked {
                    let Node::Leaf { tabs, active, .. } =
                        &mut self.dock_state[surface_index][node_index]
                    else {
                        unreachable!()
                    };
                    let tab = &mut tabs[tab_index.0];

                    if tab_viewer.on_close(tab) {
                        self.to_remove
                            .push((surface_index, node_index, tab_index).into());
                    } else {
                        *active = tab_index;
                        self.new_focused = Some((surface_index, node_index));
                    }
                }

                if let Some(pos) = state.last_hover_pos {
                    // Use response.rect.contains instead of
                    // response.hovered as the dragged tab covers
                    // the underlying tab
                    if state.drag_start.is_some() && response.rect.contains(pos) {
                        self.tab_hover_rect = Some((response.rect, tab_index));
                    }
                }

                if is_active {
                    let active_pos = response.rect.left() - tabbar_outer_rect.left();
                    let scroll_target = active_pos + response.rect.width() * tab_index.0 as f32 / tabs_len as f32;
                    active_tab_index_pos = Some((tab_index, scroll_target));
                }

                (response, title_id)
            };

            // Paint hline below each tab unless its active (or option says otherwise).
            let Node::Leaf { tabs, active, .. } = &mut self.dock_state[surface_index][node_index]
            else {
                unreachable!()
            };
            let tab = &mut tabs[tab_index.0];
            let style = fade.unwrap_or_else(|| self.style.as_ref().unwrap());
            let tab_style = tab_viewer.tab_style_override(tab, &style.tab);
            let tab_style = tab_style.as_ref().unwrap_or(&style.tab);

            if !is_active || tab_style.hline_below_active_tab_name {
                let px = tabs_ui.ctx().pixels_per_point().recip();
                tabs_ui.painter().hline(
                    response.rect.x_range(),
                    tabbar_outer_rect.bottom() - px,
                    (px, style.tab_bar.hline_color),
                );
            }

            if response.clicked()
                || (tabs_ui.memory(|m| m.has_focus(title_id))
                    && tabs_ui.input(|i| i.key_pressed(Key::Enter) || i.key_pressed(Key::Space)))
            {
                *active = tab_index;
                self.new_focused = Some((surface_index, node_index));
            }

            if self.show_close_buttons && tab_viewer.closeable(tab) && response.middle_clicked() {
                if tab_viewer.on_close(tab) {
                    self.to_remove
                        .push((surface_index, node_index, tab_index).into());
                } else {
                    *active = tab_index;
                    self.new_focused = Some((surface_index, node_index));
                }
            }

            tab_viewer.on_tab_button(tab, &response);
        }

        active_tab_index_pos
    }

    /// Draws the tab add button.
    #[allow(clippy::too_many_arguments)]
    fn tab_plus(
        &mut self,
        ui: &mut Ui,
        surface_index: SurfaceIndex,
        node_index: NodeIndex,
        tab_viewer: &mut impl TabViewer<Tab = Tab>,
        tabbar_outer_rect: Rect,
        offset: f32,
        fade_style: Option<&Style>,
    ) {
        let rect = Rect::from_min_max(
            tabbar_outer_rect.right_top() - vec2(Style::TAB_ADD_BUTTON_SIZE + offset, 0.0),
            tabbar_outer_rect.right_bottom() - vec2(offset, 2.0),
        );

        let ui = &mut ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center))
                .id_salt((node_index, "tab_add")),
        );

        let (rect, mut response) = ui.allocate_exact_size(ui.available_size(), Sense::click());

        response = response.on_hover_cursor(CursorIcon::PointingHand);

        let style = fade_style.unwrap_or_else(|| self.style.as_ref().unwrap());
        let color = if response.hovered() || response.has_focus() {
            ui.painter()
                .rect_filled(rect, Rounding::ZERO, style.buttons.add_tab_bg_fill);
            style.buttons.add_tab_active_color
        } else {
            style.buttons.add_tab_color
        };

        let mut plus_rect = rect;

        rect_set_size_centered(&mut plus_rect, Vec2::splat(Style::TAB_ADD_PLUS_SIZE));

        tab_viewer.draw_add_button(ui, style, color, rect, plus_rect);

        let popup_id = ui.id().with("tab_add_popup");
        popup_under_widget(ui, popup_id, &response, |ui| {
            tab_viewer.add_popup(ui, surface_index, node_index);
        });

        if response.clicked() {
            if self.show_add_popup {
                ui.memory_mut(|mem| mem.toggle_popup(popup_id));
            }
            tab_viewer.on_add(surface_index, node_index);
        }
    }

    /// Draws the close all button.
    #[allow(clippy::too_many_arguments)]
    #[allow(unused_assignments)]
    fn tab_close_all(
        &mut self,
        ui: &mut Ui,
        surface_index: SurfaceIndex,
        node_index: NodeIndex,
        tabbar_outer_rect: Rect,
        fade_style: Option<&Style>,
        disabled: bool,
        close_window_disabled: bool,
    ) {
        let rect = Rect::from_min_max(
            tabbar_outer_rect.right_top() - vec2(Style::TAB_CLOSE_ALL_BUTTON_SIZE, 0.0),
            tabbar_outer_rect.right_bottom() - vec2(0.0, 2.0),
        );

        let ui = &mut ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center))
                .id_salt((node_index, "tab_close_all")),
        );

        let (rect, mut response) = ui.allocate_exact_size(ui.available_size(), Sense::click());

        let style = fade_style.unwrap_or_else(|| self.style.as_ref().unwrap());

        // Whether we're on "secondary button mode" due to modifier keys
        let on_secondary_button = self.is_on_secondary_button(surface_index, ui, &response);

        let mut stroke_color = if disabled {
            style.buttons.close_all_tabs_disabled_color
        } else if response.hovered() || response.has_focus() {
            if !(close_window_disabled && on_secondary_button) {
                ui.painter().rect_filled(
                    rect,
                    Rounding::ZERO,
                    style.buttons.close_all_tabs_bg_fill,
                );
            }
            style.buttons.close_all_tabs_active_color
        } else {
            style.buttons.close_all_tabs_color
        };

        let mut close_all_rect = rect;

        rect_set_size_centered(&mut close_all_rect, Vec2::splat(Style::TAB_CLOSE_ALL_SIZE));

        if !disabled {
            response = response.on_hover_cursor(CursorIcon::PointingHand);
        }

        if on_secondary_button {
            // Close the entire window
            if close_window_disabled {
                stroke_color = style.buttons.close_all_tabs_disabled_color;
                response = response
                    .on_hover_cursor(CursorIcon::NotAllowed)
                    .on_hover_text(
                        self.dock_state
                            .translations
                            .leaf
                            .close_all_button_disabled_tooltip
                            .as_str(),
                    );
            }
            Self::draw_close_window_symbol(ui, stroke_color, close_all_rect);
        } else {
            // Close all tabs in this leaf
            if !disabled {
                if !surface_index.is_main() && self.secondary_button_context_menu {
                    response.context_menu(|ui| {
                        ui.add_enabled_ui(!close_window_disabled, |ui| {
                            if ui
                                .button(&self.dock_state.translations.leaf.close_all_button)
                                .on_disabled_hover_text(
                                    self.dock_state
                                        .translations
                                        .leaf
                                        .close_all_button_disabled_tooltip
                                        .as_str(),
                                )
                                .clicked()
                            {
                                self.to_remove.push(TabRemoval::Window(surface_index));
                            }
                        });
                    });
                }
            } else {
                response = response
                    .on_hover_cursor(CursorIcon::NotAllowed)
                    .on_hover_text(
                        self.dock_state
                            .translations
                            .leaf
                            .close_button_disabled_tooltip
                            .as_str(),
                    );
            }

            if response.clicked() {
                if on_secondary_button {
                    if !close_window_disabled {
                        self.to_remove.push(TabRemoval::Window(surface_index));
                    }
                } else if !disabled {
                    self.to_remove.push((surface_index, node_index).into());
                }
            }

            ui.painter().line_segment(
                [close_all_rect.left_top(), close_all_rect.right_bottom()],
                Stroke::new(1.0, stroke_color),
            );
            ui.painter().line_segment(
                [close_all_rect.right_top(), close_all_rect.left_bottom()],
                Stroke::new(1.0, stroke_color),
            );
        }

        // Draw button left border.
        ui.painter().vline(
            rect.left(),
            rect.y_range(),
            Stroke::new(
                ui.ctx().pixels_per_point().recip(),
                style.buttons.close_all_tabs_border_color,
            ),
        );

        if !disabled && !on_secondary_button {
            response = self.show_tooltip_hints(surface_index, response);
        }
    }

    /// Draws the collapse button.
    fn tab_collapse(
        &mut self,
        ui: &mut Ui,
        surface_index: SurfaceIndex,
        node_index: NodeIndex,
        tabbar_outer_rect: Rect,
        fade_style: Option<&Style>,
        collapsed: bool,
    ) {
        let rect = Rect::from_min_max(
            tabbar_outer_rect.left_top(),
            tabbar_outer_rect.left_bottom() + vec2(Style::TAB_COLLAPSE_BUTTON_SIZE, 0.0),
        );

        let ui = &mut ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center))
                .id_salt((node_index, "tab_collapse")),
        );

        let (rect, mut response) = ui.allocate_exact_size(ui.available_size(), Sense::click());

        response = response.on_hover_cursor(CursorIcon::PointingHand);

        let style = fade_style.unwrap_or_else(|| self.style.as_ref().unwrap());

        // Whether we're on "secondary button mode" due to modifier keys
        let on_secondary_button = self.is_on_secondary_button(surface_index, ui, &response);

        let color = if response.hovered() || response.has_focus() {
            ui.painter()
                .rect_filled(rect, Rounding::ZERO, style.buttons.collapse_tabs_bg_fill);
            style.buttons.collapse_tabs_active_color
        } else {
            style.buttons.collapse_tabs_color
        };

        let mut arrow_rect = rect;
        rect_set_size_centered(&mut arrow_rect, Vec2::splat(Style::TAB_COLLAPSE_ARROW_SIZE));

        if on_secondary_button {
            // Collapse the entire window
            Self::draw_chevron_down(ui, style, color, arrow_rect);
        } else {
            // Draw arrow.
            Self::draw_arrow(collapsed, ui, color, arrow_rect);
        }

        // Draw button right border.
        ui.painter().vline(
            rect.right(),
            rect.y_range(),
            Stroke::new(
                ui.ctx().pixels_per_point().recip(),
                style.buttons.collapse_tabs_border_color,
            ),
        );

        if response.clicked() {
            if on_secondary_button {
                self.window_toggle_minimized(surface_index);
            } else {
                self.dock_state[surface_index][node_index].set_collapsed(!collapsed);
                self.dock_state[surface_index].node_update_collapsed(node_index);
                self.window_update_collapsed(surface_index, node_index);
            }
        }

        if !surface_index.is_main() && self.secondary_button_context_menu {
            response.context_menu(|ui| {
                if ui
                    .button(&self.dock_state.translations.leaf.minimize_button)
                    .clicked()
                {
                    ui.close_menu();
                    self.window_toggle_minimized(surface_index);
                }
            });
        }

        if !on_secondary_button {
            self.show_tooltip_hints(surface_index, response);
        }
    }

    fn show_tooltip_hints(&mut self, surface_index: SurfaceIndex, response: Response) -> Response {
        if !surface_index.is_main()
            && self.show_secondary_button_hint
            && (self.secondary_button_context_menu || self.secondary_button_on_modifier)
        {
            let hint = if self.secondary_button_context_menu && self.secondary_button_on_modifier {
                &self
                    .dock_state
                    .translations
                    .leaf
                    .minimize_button_modifier_menu_hint
            } else if self.secondary_button_context_menu {
                &self.dock_state.translations.leaf.minimize_button_menu_hint
            } else {
                &self
                    .dock_state
                    .translations
                    .leaf
                    .minimize_button_modifier_hint
            };
            return response.on_hover_text(hint);
        }
        response
    }

    fn is_on_secondary_button(
        &self,
        surface_index: SurfaceIndex,
        ui: &mut Ui,
        response: &Response,
    ) -> bool {
        !surface_index.is_main()
            && self.secondary_button_on_modifier
            && ui.input(|i| {
                i.modifiers
                    .matches_logically(self.secondary_button_modifiers)
            })
            && (response.hovered() || response.has_focus() || response.is_pointer_button_down_on())
    }

    fn draw_close_window_symbol(ui: &mut Ui, stroke_color: Color32, close_all_rect: Rect) {
        ui.painter().add(Shape::line(
            vec![
                close_all_rect
                    .right_center()
                    .lerp(close_all_rect.right_bottom(), 0.5),
                close_all_rect.right_bottom(),
                close_all_rect.left_bottom(),
                close_all_rect.left_top(),
                close_all_rect
                    .center_top()
                    .lerp(close_all_rect.left_top(), 0.5),
            ],
            Stroke::new(1.0, stroke_color),
        ));
        ui.painter().line_segment(
            [close_all_rect.center_top(), close_all_rect.right_center()],
            Stroke::new(1.0, stroke_color),
        );
        ui.painter().line_segment(
            [close_all_rect.center(), close_all_rect.right_top()],
            Stroke::new(1.0, stroke_color),
        );
    }

    fn draw_arrow(collapsed: bool, ui: &mut Ui, color: Color32, arrow_rect: Rect) {
        ui.painter().add(Shape::convex_polygon(
            if collapsed {
                // Arrow pointing rightwards.
                vec![
                    arrow_rect.left_top(),
                    arrow_rect.right_center(),
                    arrow_rect.left_bottom(),
                ]
            } else {
                // Arrow pointing downwards.
                vec![
                    arrow_rect.left_top(),
                    arrow_rect.right_top(),
                    arrow_rect.center_bottom(),
                ]
            },
            color,
            Stroke::NONE,
        ));
    }

    fn draw_chevron_down(ui: &mut Ui, style: &Style, color: Color32, arrow_rect: Rect) {
        ui.painter().add(Shape::convex_polygon(
            // Arrow pointing downwards.
            vec![
                arrow_rect.left_top(),
                arrow_rect.right_top(),
                arrow_rect.center(),
            ],
            color,
            Stroke::NONE,
        ));

        // Chevron pointing downwards.
        ui.painter().add(Shape::convex_polygon(
            vec![
                arrow_rect.left_center(),
                arrow_rect.right_center(),
                arrow_rect.center_bottom(),
            ],
            color,
            Stroke::NONE,
        ));
        let color = style.buttons.minimize_window_bg_fill;
        ui.painter().add(Shape::convex_polygon(
            vec![
                arrow_rect
                    .left_center()
                    .lerp(arrow_rect.right_center(), 0.25),
                arrow_rect
                    .left_center()
                    .lerp(arrow_rect.right_center(), 0.75),
                arrow_rect.center().lerp(arrow_rect.center_bottom(), 0.5),
            ],
            color,
            Stroke::NONE,
        ));
    }

    /// Updates the collapsed state of the node and its parents.
    fn window_update_collapsed(&mut self, surface_index: SurfaceIndex, node_index: NodeIndex) {
        let surface = &mut self.dock_state[surface_index];
        let collapsed = surface[node_index].is_collapsed();
        if !collapsed {
            if let Some(window_state) = self.dock_state.get_window_state_mut(surface_index) {
                window_state.set_new(true);
            }
        } else if surface.root_node().is_some_and(|root| root.is_collapsed()) {
            let root_index = NodeIndex::root();
            let surface_height = if surface.root_node().is_some() {
                surface[root_index].rect().unwrap().height()
            } else {
                0.0
            };
            if let Some(window_state) = self.dock_state.get_window_state_mut(surface_index) {
                window_state.set_expanded_height(surface_height);
            }
        }
    }

    /// * `active` means "the tab that is opened in the parent panel".
    /// * `focused` means "the tab that was last interacted with".
    ///
    /// Returns the main button response plus the response of the close button, if any.
    #[allow(clippy::too_many_arguments)]
    fn tab_title(
        &mut self,
        ui: &mut Ui,
        tab_style: &TabStyle,
        id: Id,
        label: WidgetText,
        focused: bool,
        active: bool,
        is_being_dragged: bool,
        preferred_width: Option<f32>,
        show_close_button: bool,
        fade: Option<&Style>,
    ) -> (Response, Option<Response>) {
        let style = fade.unwrap_or_else(|| self.style.as_ref().unwrap());
        let galley = label.into_galley(ui, None, f32::INFINITY, TextStyle::Button);
        let x_spacing = 8.0;
        let text_width = galley.size().x + 2.0 * x_spacing;
        let close_button_size = if show_close_button {
            Style::TAB_CLOSE_BUTTON_SIZE.min(style.tab_bar.height)
        } else {
            0.0
        };

        // Compute total width of the tab bar.
        let minimum_width = tab_style
            .minimum_width
            .unwrap_or(0.0)
            .at_least(text_width + close_button_size);
        let tab_width = preferred_width.unwrap_or(0.0).at_least(minimum_width);

        let (_, tab_rect) = ui.allocate_space(vec2(tab_width, ui.available_height()));
        let mut response = ui.interact(tab_rect, id, Sense::click_and_drag());
        if ui.ctx().dragged_id().is_none() && self.draggable_tabs {
            response = response.on_hover_cursor(CursorIcon::Grab);
        }

        let tab_style = if focused || is_being_dragged {
            if response.has_focus() {
                &tab_style.focused_with_kb_focus
            } else {
                &tab_style.focused
            }
        } else if response.hovered() {
            &tab_style.hovered
        } else if active {
            if response.has_focus() {
                &tab_style.active_with_kb_focus
            } else {
                &tab_style.active
            }
        } else if response.has_focus() {
            &tab_style.inactive_with_kb_focus
        } else {
            &tab_style.inactive
        };

        // Draw the full tab first and then the stroke on top to avoid the stroke
        // mixing with the background color.
        ui.painter()
            .rect_filled(tab_rect, tab_style.rounding, tab_style.bg_fill);
        let stroke_rect = rect_stroke_box(tab_rect, 1.0);
        ui.painter().rect_stroke(
            stroke_rect,
            tab_style.rounding,
            Stroke::new(1.0, tab_style.outline_color),
        );
        if !is_being_dragged {
            // Make the tab name area connect with the tab ui area.
            ui.painter().hline(
                RangeInclusive::new(
                    stroke_rect.min.x + f32::max(tab_style.rounding.sw, 1.5),
                    stroke_rect.max.x - f32::max(tab_style.rounding.se, 1.5),
                ),
                stroke_rect.bottom(),
                Stroke::new(2.0, tab_style.bg_fill),
            );
        }

        let mut text_rect = tab_rect;
        text_rect.set_width(text_rect.width() - close_button_size);
        let text_pos = {
            let pos = Align2::CENTER_CENTER.pos_in_rect(&text_rect.shrink2(vec2(x_spacing, 0.0)));
            pos - galley.size() / 2.0
        };

        ui.painter()
            .add(TextShape::new(text_pos, galley, tab_style.text_color));

        let close_response = show_close_button.then(|| {
            let mut close_button_rect = tab_rect;
            close_button_rect.set_left(text_rect.right());
            close_button_rect =
                Rect::from_center_size(close_button_rect.center(), Vec2::splat(close_button_size));

            let close_response = ui
                .interact(close_button_rect, id.with("close-button"), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand);

            let color = if close_response.hovered() || close_response.has_focus() {
                style.buttons.close_tab_active_color
            } else {
                style.buttons.close_tab_color
            };

            if close_response.hovered() || close_response.has_focus() {
                let mut rounding = tab_style.rounding;
                rounding.nw = 0.0;
                rounding.sw = 0.0;

                ui.painter().rect_filled(
                    close_button_rect,
                    rounding,
                    style.buttons.add_tab_bg_fill,
                );
            }

            let mut x_rect = close_button_rect;
            rect_set_size_centered(&mut x_rect, Vec2::splat(Style::TAB_CLOSE_X_SIZE));
            ui.painter().line_segment(
                [x_rect.left_top(), x_rect.right_bottom()],
                Stroke::new(1.0, color),
            );
            ui.painter().line_segment(
                [x_rect.right_top(), x_rect.left_bottom()],
                Stroke::new(1.0, color),
            );

            close_response
        });

        (response, close_response)
    }

    #[allow(clippy::too_many_arguments)]
    fn tab_bar_scroll(
        &mut self,
        ui: &mut Ui,
        state: &State,
        (surface_index, node_index): (SurfaceIndex, NodeIndex),
        actual_width: f32,
        available_width: f32,
        scroll_bar_width: f32,
        tabbar_response: &Response,
        fade_style: Option<&Style>,
        tab_scroll_target: &mut Option<f32>,
    ) {
        // assert_ne!(available_width, 0.0);
        let available_width = available_width.max(1.0);

        let Node::Leaf { scroll, .. } = &mut self.dock_state[surface_index][node_index] else {
            unreachable!()
        };
        let overflow = (actual_width - available_width).at_least(0.0);
        let style = fade_style.unwrap_or_else(|| self.style.as_ref().unwrap());

        // Compare to 1.0 and not 0.0 to avoid drawing a scroll bar due
        // to floating point precision issue during tab drawing.
        let mut had_scroll = false;
        let mut user_driven = false;
        if overflow > 1.0 {
            if style.tab_bar.show_scroll_bar_on_overflow {
                had_scroll = true;

                // Draw scroll bar
                let bar_height = 7.5;
                let (scroll_bar_rect, _scroll_bar_response) = ui.allocate_exact_size(
                    vec2(scroll_bar_width, bar_height),
                    Sense::click_and_drag(),
                );

                // Compute scroll bar handle position and size.
                let overflow_ratio = actual_width / available_width;
                let scroll_ratio = -*scroll / overflow;

                let scroll_bar_handle_size = overflow_ratio.recip() * scroll_bar_rect.width();
                let scroll_bar_handle_start = lerp(
                    scroll_bar_rect.left()..=scroll_bar_rect.right() - scroll_bar_handle_size,
                    scroll_ratio,
                );
                let scroll_bar_handle_rect = Rect::from_min_size(
                    pos2(scroll_bar_handle_start, scroll_bar_rect.min.y),
                    vec2(scroll_bar_handle_size, bar_height),
                );

                let scroll_bar_handle_response = ui.interact(
                    scroll_bar_handle_rect,
                    self.id.with((node_index, "node")),
                    Sense::drag(),
                );

                // Coefficient to apply to input displacements so that we move the scroll by the correct amount.
                let points_to_scroll_coefficient =
                    overflow / (scroll_bar_rect.width() - scroll_bar_handle_size);

                *scroll -= scroll_bar_handle_response.drag_delta().x * points_to_scroll_coefficient;

                if let Some(pos) = state.last_hover_pos {
                    if scroll_bar_rect.contains(pos) {
                        *scroll += ui.input(|i| i.smooth_scroll_delta.y + i.smooth_scroll_delta.x)
                            * points_to_scroll_coefficient;
                    }
                }

                // Draw the bar.
                ui.painter()
                    .rect_filled(scroll_bar_rect, 0.0, ui.visuals().extreme_bg_color);

                ui.painter().rect_filled(
                    scroll_bar_handle_rect,
                    bar_height / 2.0,
                    ui.visuals()
                        .widgets
                        .style(&scroll_bar_handle_response)
                        .bg_fill,
                );
            }

            // Handle user input.
            if ui.rect_contains_pointer(tabbar_response.rect) {
                let delta = ui.input(|i| i.smooth_scroll_delta.y + i.smooth_scroll_delta.x);
                *scroll += delta;
                user_driven = delta.abs() > 1.0;
            }
        }

        // Animate to active tab.
        if let Some(active_pos) = tab_scroll_target.map(|x| x) {
            if !had_scroll || user_driven {
                *tab_scroll_target = None;
            } else {
                let new_scroll = -active_pos.min(available_width);
                let anim = *scroll * 0.9 + new_scroll * 0.1;
                if (anim - new_scroll).abs() < 1.0 {
                    *tab_scroll_target = None;
                }
                *scroll = anim;
            }
        }

        *scroll = scroll.clamp(-overflow, 0.0);
    }

    #[allow(clippy::too_many_arguments)]
    fn tab_body(
        &mut self,
        ui: &mut Ui,
        state: &State,
        (surface_index, node_index): (SurfaceIndex, NodeIndex),
        tab_viewer: &mut impl TabViewer<Tab = Tab>,
        spacing: Vec2,
        tabbar_rect: Rect,
        fade: Option<(&Style, f32)>,
        collapsed: bool,
    ) {
        let (body_rect, _body_response) =
            ui.allocate_exact_size(ui.available_size_before_wrap(), Sense::hover());

        let Node::Leaf {
            rect,
            tabs,
            active,
            viewport,
            ..
        } = &mut self.dock_state[surface_index][node_index]
        else {
            unreachable!();
        };

        if !collapsed {
            if let Some(tab) = tabs.get_mut(active.0) {
                *viewport = body_rect;

                if ui.input(|i| i.pointer.any_click()) {
                    if let Some(pos) = state.last_hover_pos {
                        if body_rect.contains(pos)
                            && Some(ui.layer_id()) == ui.ctx().layer_id_at(pos)
                        {
                            self.new_focused = Some((surface_index, node_index));
                        }
                    }
                }

                let (style, fade_factor) =
                    fade.unwrap_or_else(|| (self.style.as_ref().unwrap(), 1.0));
                let tabs_styles = tab_viewer.tab_style_override(tab, &style.tab);

                let tabs_style = tabs_styles.as_ref().unwrap_or(&style.tab);

                if tab_viewer.clear_background(tab) {
                    ui.painter().rect_filled(
                        body_rect,
                        tabs_style.tab_body.rounding,
                        tabs_style.tab_body.bg_fill,
                    );
                }

                // Construct a new ui with the correct tab id.
                //
                // We are forced to use `Ui::new` because other methods (eg: push_id) always mix
                // the provided id with their own which would cause tabs to change id when moved
                // from node to node.
                let id = self.id.with(tab_viewer.id(tab));
                ui.ctx().check_for_id_clash(id, body_rect, "a tab with id");
                let ui = &mut Ui::new(
                    ui.ctx().clone(),
                    id,
                    UiBuilder::new().max_rect(body_rect).layer_id(ui.layer_id()),
                );
                ui.set_clip_rect(Rect::from_min_max(ui.cursor().min, ui.clip_rect().max));

                // Use initial spacing for ui.
                ui.spacing_mut().item_spacing = spacing;

                // Offset the background rectangle up to hide the top border behind the clip rect.
                // To avoid anti-aliasing lines when the stroke width is not divisible by two, we
                // need to calculate the effective anti-aliased stroke width.
                let effective_stroke_width = (tabs_style.tab_body.stroke.width / 2.0).ceil() * 2.0;
                let tab_body_rect = Rect::from_min_max(
                    ui.clip_rect().min - vec2(0.0, effective_stroke_width),
                    ui.clip_rect().max,
                );
                ui.painter().rect_stroke(
                    rect_stroke_box(tab_body_rect, tabs_style.tab_body.stroke.width),
                    tabs_style.tab_body.rounding,
                    tabs_style.tab_body.stroke,
                );

                ScrollArea::new(tab_viewer.scroll_bars(tab)).show(ui, |ui| {
                    Frame::none()
                        .inner_margin(tabs_style.tab_body.inner_margin)
                        .show(ui, |ui| {
                            if fade_factor != 1.0 {
                                fade_visuals(ui.visuals_mut(), fade_factor);
                            }
                            let available_rect = ui.available_rect_before_wrap();
                            ui.expand_to_include_rect(available_rect);
                            tab_viewer.ui(ui, tab);
                        });
                });
            }
        }

        // change hover destination
        if let Some(pointer) = state.last_hover_pos {
            // Prevent borrow checker issues.
            let rect = rect.to_owned();

            // if the dragged tab isn't allowed in a window,
            // it's unnecessary to change the hover state
            let is_dragged_valid = match &state.dnd {
                Some(DragDropState {
                    drag: DragData { src, .. },
                    ..
                }) => match *src {
                    TreeComponent::Tab(d_surf, d_node, d_tab) => {
                        if let Node::Leaf { tabs, .. } = &mut self.dock_state[d_surf][d_node] {
                            tab_viewer.allowed_in_windows(&mut tabs[d_tab.0])
                                || surface_index == SurfaceIndex::main()
                        } else {
                            true
                        }
                    }
                    _ => unreachable!("collections of nodes can't be dragged (yet)"),
                },
                _ => true,
            };

            // Use rect.contains instead of response.hovered as the dragged tab covers
            // the underlying responses.
            if state.drag_start.is_some() && rect.contains(pointer) && is_dragged_valid {
                let on_title_bar = tabbar_rect.contains(pointer);
                let (dst, tab) = {
                    match self.tab_hover_rect {
                        Some((rect, tab_index)) => (
                            TreeComponent::Tab(surface_index, node_index, tab_index),
                            Some(rect),
                        ),
                        None => (
                            TreeComponent::Node(surface_index, node_index),
                            on_title_bar.then_some(tabbar_rect),
                        ),
                    }
                };

                ui.memory_mut(|mem| {
                    mem.data.insert_temp(
                        self.id.with("hover_data"),
                        Some(HoverData { rect, dst, tab }),
                    );
                });
            }
        }
    }
}
