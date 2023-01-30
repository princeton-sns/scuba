use noise_core::core::Core;

enum MessageType {
  RequestUpdateLinked,
  ConfirmUpdateLinked,
  RequestContact,
  ConfirmContact,
  LinkGroups,
  AddParent,
  AddChild,
  AddPermission,
  RemoveParent,
  RemoveChild,
  RemovePermission,
  UpdateGroup,
  UpdateData,
  DeleteGroup,
  DeleteData,
  DeleteDevice,
}

pub struct Rest {
  core: Core,
}

impl Rest {
  pub fn new<'a>(
      ip_arg: Option<&'a str>,
      port_arg: Option<&'a str>,
      turn_encryption_off_arg: bool,
  ) -> Rest {
    Self {
      core: Core::new(ip_arg, port_arg, turn_encryption_off_arg),
    }
  }
}
