use adw::subclass::prelude::ObjectSubclassIsExt;
use gtk::{glib, prelude::WidgetExt};

use super::UnitListPanel;

glib::wrapper! {
    pub struct UnitListSearchControls(ObjectSubclass<imp::UnitListSearchControlsImp>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl UnitListSearchControls {
    pub fn new(unit_list_panel: &UnitListPanel) -> UnitListSearchControls {
        let obj: UnitListSearchControls = glib::Object::new();
        obj.imp()
            .unit_list_panel
            .set(unit_list_panel.clone())
            .unwrap();
        obj.imp().set_filter_is_set(false);
        obj
    }

    pub fn grab_focus_on_search_entry(&self) {
        self.imp().search_entry.grab_focus();
    }
}

mod imp {
    use std::cell::{Cell, OnceCell};

    use gtk::{glib, prelude::*, subclass::prelude::*};

    use crate::widget::unit_list::UnitListPanel;

    use super::UnitListSearchControls;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/plrigaux/sysd-manager/unit_list_search.ui")]
    pub struct UnitListSearchControlsImp {
        #[template_child]
        pub(super) search_entry: TemplateChild<gtk::SearchEntry>,

        #[template_child]
        show_filter_button: TemplateChild<gtk::Button>,

        #[template_child]
        clear_filters_button: TemplateChild<gtk::Button>,

        filter_is_set: Cell<bool>,

        signal_handler_text_changed: OnceCell<glib::SignalHandlerId>,

        pub(super) unit_list_panel: OnceCell<UnitListPanel>,
    }

    #[gtk::template_callbacks]
    impl UnitListSearchControlsImp {
        #[template_callback]
        fn show_filters_clicked(button: &gtk::Button) {
            button
                .activate_action("win.unit_list_filter_blank", None)
                .expect("The action \"list filter\" does not exist.");
        }

        #[template_callback]
        fn clear_filter_clicked(button: &gtk::Button) {
            button
                .activate_action("win.unit_list_filter_clear", None)
                .expect("The action \"clear filter\" does not exist.");
        }

        fn update_unit_name_search(&self, text: &str) {
            self.unit_list_panel
                .get()
                .unwrap()
                .imp()
                .update_unit_name_search(text, false)
        }

        pub(crate) fn set_search_entry_text(&self, text: &str) {
            let signal_handler_id = self.signal_handler_text_changed.get().unwrap();
            self.search_entry.block_signal(signal_handler_id);
            self.search_entry.set_text(text);
            self.search_entry.unblock_signal(signal_handler_id);
        }

        pub(crate) fn clear(&self) {
            self.set_search_entry_text("");
            self.set_filter_is_set(false);
        }

        pub(crate) fn set_filter_is_set(&self, filter_is_set: bool) {
            self.filter_is_set.set(filter_is_set);
            self.clear_filters_button.set_sensitive(filter_is_set);
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for UnitListSearchControlsImp {
        const NAME: &'static str = "SEARCH_CONTROLS";
        type Type = UnitListSearchControls;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            // The layout manager determines how child widgets are laid out.
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for UnitListSearchControlsImp {
        fn constructed(&self) {
            self.parent_constructed();

            let unit_list_panel = self.obj().clone();
            let signal_handler_id = self.search_entry.connect_changed(move |entry| {
                let text = entry.text();
                unit_list_panel.imp().update_unit_name_search(text.as_str());
            });

            self.signal_handler_text_changed
                .set(signal_handler_id)
                .expect("Search entry handler set once");
        }
    }

    impl WidgetImpl for UnitListSearchControlsImp {}
    impl BoxImpl for UnitListSearchControlsImp {}
}
