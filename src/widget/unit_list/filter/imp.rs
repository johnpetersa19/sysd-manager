use std::{
    cell::{OnceCell, RefCell},
    rc::Rc,
};

use adw::{prelude::*, subclass::window::AdwWindowImpl};

use gtk::{
    glib::{self},
    subclass::{
        prelude::*,
        widget::{
            CompositeTemplateCallbacksClass, CompositeTemplateClass,
            CompositeTemplateInitializingExt, WidgetImpl,
        },
    },
};

use log::warn;
use strum::IntoEnumIterator;

use crate::{
    systemd::enums::{ActiveState, EnablementStatus, LoadState, Preset, UnitDBusLevel, UnitType},
    widget::{
        preferences::data::UNIT_LIST_COLUMNS,
        unit_list::{UnitListPanel, filter::get_filter_element_mut},
    },
};

use super::{FilterText, UnitListFilterWindow, get_filter_element};
use crate::widget::unit_list::filter::UnitPropertyFilter;
#[derive(Default, gtk::CompositeTemplate, glib::Properties)]
#[template(resource = "/io/github/plrigaux/sysd-manager/unit_list_filter.ui")]
#[properties(wrapper_type = super::UnitListFilterWindow)]
pub struct UnitListFilterWindowImp {
    #[template_child]
    filter_stack: TemplateChild<adw::ViewStack>,

    #[template_child]
    filter_navigation_container: TemplateChild<gtk::Box>,

    #[property(get, set, nullable, default = None)]
    selected: RefCell<Option<String>>,

    pub(super) unit_list_panel: OnceCell<UnitListPanel>,
}

#[gtk::template_callbacks]
impl UnitListFilterWindowImp {
    pub(super) fn get_filter(&self) {
        let unit_list_panel = self
            .unit_list_panel
            .get()
            .expect("unit_list_panel in filter dialog not None");

        let binding = self.selected.borrow();
        let selected = binding.as_ref();

        for (name, key, num_id, _) in &*UNIT_LIST_COLUMNS {
            let filter_assessor = unit_list_panel.try_get_filter_assessor(*num_id);

            let widget: gtk::Widget = if let Some(filter) = filter_assessor {
                match *key {
                    "unit" => common_text_filter(filter).into(),
                    "bus" => build_bus_level_filter(filter).into(),
                    "type" => build_type_filter(filter).into(),
                    "state" => build_enablement_filter(filter).into(),
                    "preset" => build_preset_filter(filter).into(),
                    "load" => build_load_filter(filter).into(),
                    "active" => build_active_state_filter(filter).into(),
                    "sub" => common_text_filter(filter).into(),
                    "description" => common_text_filter(filter).into(),

                    _ => unreachable!("unreachable"),
                }
            } else {
                gtk::Label::new(Some(name)).into()
            };

            let _stack_page = self.filter_stack.add_titled(&widget, Some(key), name);

            let button_content = adw::ButtonContent::builder()
                .icon_name("empty-icon")
                .label(name)
                .halign(gtk::Align::Start)
                .css_classes(["nav"])
                .build();

            if let Some(filter_container) = filter_assessor {
                let mut filter_container_binding = filter_container.as_ref().borrow_mut();
                let is_empty = filter_container_binding.is_empty();
                button_content.set_icon_name(icon_name(is_empty));
                {
                    let button_content = button_content.clone();
                    let lambda = move |is_empty: bool| {
                        let icon_name = icon_name(is_empty);
                        button_content.set_icon_name(icon_name);
                    };

                    filter_container_binding.set_on_change(Box::new(lambda));
                }
            }

            let button = gtk::Button::builder()
                .child(&button_content)
                .css_classes(["flat"])
                .build();

            if selected.is_some_and(|s| s == key) {
                button.remove_css_class("flat");
            }

            let tooltip_text = match *key {
                "load" => Some("Reflects whether the unit definition was properly loaded."),
                "active" => {
                    Some("The high-level unit activation state, i.e. generalization of <b>Sub</b>.")
                }
                "sub" => Some("The low-level unit activation state, values depend on unit type."),
                _ => None,
            };

            button.set_tooltip_markup(tooltip_text);
            {
                let filter_stack = self.filter_stack.clone();
                button.connect_clicked(move |button| {
                    set_visible_child_name(&filter_stack, key);

                    if let Some(parent) = button.parent() {
                        let mut child_o = parent.first_child();

                        while let Some(child) = child_o {
                            child.add_css_class("flat");
                            child_o = child.next_sibling();
                        }
                    }
                    button.remove_css_class("flat");
                });
            }
            self.filter_navigation_container.append(&button);
        }

        let box_pad = gtk::Box::builder().vexpand(true).build();

        let clear_filter_button = gtk::Button::builder()
            .label("Clear Filters")
            // .css_classes(["destructive-action"])
            .valign(gtk::Align::End)
            .hexpand(true)
            .build();
        {
            let unit_list_panel = unit_list_panel.clone();
            let filter_stack = self.filter_stack.clone();
            clear_filter_button.connect_clicked(move |_b| {
                for (_, _, num_id, _) in &*UNIT_LIST_COLUMNS {
                    unit_list_panel.filter_assessor_change(
                        *num_id,
                        None,
                        Some(gtk::FilterChange::LessStrict),
                        true,
                    );
                }

                let sel = filter_stack.pages();
                let list: gio::ListModel = sel.into();

                let nb = list.n_items();
                for position in 0..nb {
                    let Some(object) = list.item(position) else {
                        warn!("No item at position {position}");
                        continue;
                    };

                    let Ok(page) = object.downcast::<adw::ViewStackPage>() else {
                        warn!("Not a view stack page");
                        continue;
                    };

                    let container = page.child();

                    fn clear(mut some_widget: Option<gtk::Widget>) {
                        while let Some(w) = some_widget.as_ref() {
                            if let Some(check) = w.downcast_ref::<gtk::CheckButton>() {
                                check.set_active(false);
                            } else if let Some(entry) = w.downcast_ref::<gtk::Entry>() {
                                entry.set_text("");
                            }

                            clear(w.first_child());
                            some_widget = w.next_sibling();
                        }
                    }

                    clear(container.first_child())
                }
            });
        }

        box_pad.append(&clear_filter_button);
        self.filter_navigation_container.append(&box_pad);

        if let Some(selected) = selected {
            set_visible_child_name(&self.filter_stack, selected);
        }

        self.obj()
            .bind_property::<adw::ViewStack>(
                "selected",
                self.filter_stack.as_ref(),
                "visible-child-name",
            )
            .bidirectional()
            .build();
    }
}

fn set_visible_child_name(filter_stack: &adw::ViewStack, name: &str) {
    filter_stack.set_visible_child_name(name);
    let widget = filter_stack.child_by_name(name);
    if let Some(widget) = widget {
        grab_focus_on_child_entry(widget.first_child().as_ref());
    }
}

fn grab_focus_on_child_entry(widget: Option<&gtk::Widget>) {
    let Some(widget) = widget else {
        return;
    };

    if let Some(entry) = widget.downcast_ref::<gtk::Entry>() {
        entry.grab_focus();
        return;
    }

    grab_focus_on_child_entry(widget.first_child().as_ref());

    grab_focus_on_child_entry(widget.next_sibling().as_ref());
}

fn icon_name(is_empty: bool) -> &'static str {
    if is_empty {
        "empty-icon"
    } else {
        "funnel-symbolic"
    }
}

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for UnitListFilterWindowImp {
    const NAME: &'static str = "UNIT_LIST_FILTER";
    type Type = UnitListFilterWindow;
    type ParentType = adw::Window;

    fn class_init(klass: &mut Self::Class) {
        // The layout manager determines how child widgets are laid out.
        klass.bind_template();
        klass.bind_template_callbacks();
    }

    fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
        obj.init_template();
    }
}

#[glib::derived_properties]
impl ObjectImpl for UnitListFilterWindowImp {
    fn constructed(&self) {
        self.parent_constructed();
    }
}

impl WidgetImpl for UnitListFilterWindowImp {}
impl WindowImpl for UnitListFilterWindowImp {
    // Save window state right before the window will be closed
    fn close_request(&self) -> glib::Propagation {
        self.unit_list_panel
            .get()
            .expect("Not None")
            .clear_unit_list_filter_window_dependancy();

        self.parent_close_request();
        // Allow to invoke other event handlers
        glib::Propagation::Proceed
    }
}
impl AdwWindowImpl for UnitListFilterWindowImp {}

fn common_text_filter(filter_container: &Rc<RefCell<Box<dyn UnitPropertyFilter>>>) -> gtk::Box {
    let container = create_content_box();

    let merge_box = gtk::Box::builder()
        .css_classes(["linked"])
        .halign(gtk::Align::BaselineFill)
        .build();
    let entry = gtk::Entry::builder().hexpand(true).build();
    let button_clear_entry = gtk::Button::builder()
        .icon_name("edit-clear-symbolic")
        .build();
    merge_box.append(&entry);
    merge_box.append(&button_clear_entry);
    container.append(&merge_box);

    let filter_container = filter_container.clone();

    {
        let filter_container = filter_container.borrow();
        entry.set_text(filter_container.text());
    }
    entry.connect_changed(move |entry| {
        let text = entry.text();

        let mut binding = filter_container.as_ref().borrow_mut();

        let filter_text = binding
            .as_any_mut()
            .downcast_mut::<FilterText>()
            .expect("downcast_mut to FilterText");
        filter_text.set_filter_elem(&text, true);
    });

    button_clear_entry.connect_clicked(move |_| {
        entry.set_text("");
    });

    container
}

fn build_type_filter(filter_container: &Rc<RefCell<Box<dyn UnitPropertyFilter>>>) -> gtk::Box {
    let container = create_content_box();

    //  let filter_elem = Rc::new(RefCell::new(FilterElem::default()));
    for unit_type in UnitType::iter().filter(|x| !matches!(*x, UnitType::Unknown(_))) {
        let check = {
            let binding = filter_container.borrow();
            let active = get_filter_element::<String>(binding.as_ref())
                .contains(&unit_type.as_str().to_owned());

            gtk::CheckButton::builder()
                .label(unit_type.as_str())
                .active(active)
                .build()
        };

        let filter_elem = filter_container.clone();
        check.connect_toggled(move |check_button| {
            //println!("t {} {:?}", check_button.is_active(), unit_type.as_str());
            let mut filter_elem = filter_elem.borrow_mut();
            let filter_element = get_filter_element_mut::<String>(filter_elem.as_mut());
            filter_element.set_filter_elem(unit_type.as_str().to_owned(), check_button.is_active());
        });

        container.append(&check);
    }

    build_controls(&container);

    container
}

macro_rules! build_elem_filter {
    ($filter_container:expr, $iter:expr,$value_type:ty) => {{
        let container = create_content_box();

        //  let filter_elem = Rc::new(RefCell::new(FilterElem::default()));
        for value in $iter {
            let check = {
                let binding = $filter_container.borrow();
                let active = get_filter_element::<$value_type>(binding.as_ref()).contains(&value);

                let label = gtk::Label::builder()
                    .label(value.label())
                    .use_markup(true)
                    .build();

                gtk::CheckButton::builder()
                    .child(&label)
                    .active(active)
                    .build()
            };

            check.set_tooltip_markup(value.tooltip_info().as_deref());

            let filter_elem = $filter_container.clone();
            check.connect_toggled(move |check_button| {
                let mut filter_elem = filter_elem.borrow_mut();
                let filter_element = get_filter_element_mut::<$value_type>(filter_elem.as_mut());
                filter_element.set_filter_elem(value, check_button.is_active());
            });

            container.append(&check);
        }

        build_controls(&container);

        container
    }};
}

fn build_preset_filter(filter_container: &Rc<RefCell<Box<dyn UnitPropertyFilter>>>) -> gtk::Box {
    build_elem_filter!(filter_container, Preset::iter(), Preset)
}

fn build_bus_level_filter(filter_container: &Rc<RefCell<Box<dyn UnitPropertyFilter>>>) -> gtk::Box {
    build_elem_filter!(filter_container, UnitDBusLevel::iter(), UnitDBusLevel)
}

fn create_content_box() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .width_request(300)
        .spacing(5)
        .margin_start(5)
        .margin_top(5)
        .margin_end(5)
        .build()
}

fn build_controls(container: &gtk::Box) {
    let controls = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .css_classes(["linked"])
        .halign(gtk::Align::Center)
        .build();

    let clear_button = gtk::Button::builder().label("Clear").build();
    {
        let container = container.clone();
        clear_button.connect_clicked(move |_| {
            let mut some_widget = container.first_child();
            while let Some(w) = some_widget.as_ref() {
                let maybe_checkbutton = w.downcast_ref::<gtk::CheckButton>();
                if let Some(check) = maybe_checkbutton {
                    check.set_active(false);
                }

                some_widget = w.next_sibling();
            }
        });
    }

    let inv_button = gtk::Button::builder().label("Invert").build();
    {
        let container = container.clone();
        inv_button.connect_clicked(move |_| {
            let mut some_widget = container.first_child();

            while let Some(w) = some_widget.as_ref() {
                let maybe_checkbutton = w.downcast_ref::<gtk::CheckButton>();
                if let Some(check) = maybe_checkbutton {
                    check.set_active(!check.is_active());
                }

                some_widget = w.next_sibling();
            }
        });
    }
    controls.append(&clear_button);
    controls.append(&inv_button);

    container.append(&controls);
}

fn build_enablement_filter(
    filter_container: &Rc<RefCell<Box<dyn UnitPropertyFilter>>>,
) -> gtk::Box {
    build_elem_filter!(filter_container, EnablementStatus::iter(), EnablementStatus)
}

fn build_load_filter(filter_container: &Rc<RefCell<Box<dyn UnitPropertyFilter>>>) -> gtk::Box {
    build_elem_filter!(filter_container, LoadState::iter(), LoadState)
}

fn build_active_state_filter(
    filter_container: &Rc<RefCell<Box<dyn UnitPropertyFilter>>>,
) -> gtk::Box {
    build_elem_filter!(filter_container, ActiveState::iter(), ActiveState)
}
