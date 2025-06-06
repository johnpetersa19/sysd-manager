use std::cell::Ref;

use crate::{gtk::subclass::prelude::*, systemd::data::UnitInfo};
use gtk::glib::{self, Binding};

glib::wrapper! {
    pub struct UnitBinding(ObjectSubclass<imp::UnitBindingImpl>);
}

impl UnitBinding {
    pub fn new(unit: &UnitInfo) -> Self {
        let this_object: Self = glib::Object::new();
        this_object.imp().unit.replace(unit.clone());
        this_object
    }

    pub fn unit(&self) -> UnitInfo {
        self.imp().unit.borrow().clone()
    }

    pub fn unit_ref(&self) -> Ref<'_, UnitInfo> {
        self.imp().unit.borrow()
    }

    pub fn set_binding(&self, id: u8, binding: Binding) {
        self.imp().set_binding(id, binding);
    }

    pub fn unset_binding(&self, id: u8) {
        self.imp().unset_binding(id);
    }

    pub fn primary(&self) -> String {
        self.imp().unit.borrow().primary()
    }
}

mod imp {

    use std::{cell::RefCell, collections::HashMap};

    use gtk::{
        glib::{self, Binding},
        subclass::prelude::*,
    };

    use crate::systemd::data::UnitInfo;

    #[derive(Debug, Default)]
    pub struct UnitBindingImpl {
        pub unit: RefCell<UnitInfo>,
        pub bindings: RefCell<HashMap<u8, Binding>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for UnitBindingImpl {
        const NAME: &'static str = "UnitBinding";
        type Type = super::UnitBinding;

        fn new() -> Self {
            Default::default()
        }
    }

    impl ObjectImpl for UnitBindingImpl {}

    impl UnitBindingImpl {
        pub fn set_binding(&self, id: u8, binding: Binding) {
            let mut bindings = self.bindings.borrow_mut();

            if let Some(old_binding) = bindings.insert(id, binding) {
                old_binding.unbind();
            }
        }

        pub fn unset_binding(&self, id: u8) {
            let mut bindings = self.bindings.borrow_mut();

            if let Some(old_binding) = bindings.remove(&id) {
                old_binding.unbind();
            }
        }
    }
}
