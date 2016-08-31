
use cpp_method::{CppMethod, CppMethodKind, CppMethodClassMembership, CppFunctionArgument};
use cpp_operator::CppOperator;
use std::collections::HashSet;
use log;
use cpp_type::{CppType, CppTypeBase, CppTypeIndirection, CppTypeClassBase};

pub use serializable::{EnumValue, CppClassField, CppTypeKind, CppOriginLocation, CppVisibility,
                       CppTypeData, CppData, CppTemplateInstantiation};

fn apply_instantiations_to_method(method: &CppMethod,
                                  nested_level: i32,
                                  template_instantiations: &Vec<CppTemplateInstantiation>)
                                  -> Result<Vec<CppMethod>, String> {
  let mut new_methods = Vec::new();
  for ins in template_instantiations {
    log::noisy(format!("instantiation: {:?}", ins.template_arguments));
    let mut new_method = method.clone();
    new_method.arguments.clear();
    for arg in &method.arguments {
      new_method.arguments.push(CppFunctionArgument {
        name: arg.name.clone(),
        has_default_value: arg.has_default_value,
        argument_type: try!(arg.argument_type
          .instantiate(nested_level, &ins.template_arguments)),
      });
    }
    new_method.return_type = try!(method.return_type
      .instantiate(nested_level, &ins.template_arguments));
    if let Some(ref mut info) = new_method.class_membership {
      info.class_type = try!(info.class_type
        .instantiate_class(nested_level, &ins.template_arguments));
    }
    let mut conversion_type = None;
    if let Some(ref mut operator) = new_method.operator {
      if let &mut CppOperator::Conversion(ref mut cpp_type) = operator {
        let r = try!(cpp_type.instantiate(nested_level, &ins.template_arguments));
        *cpp_type = r.clone();
        conversion_type = Some(r);
      }
    }
    if new_method.all_involved_types()
      .iter()
      .find(|t| t.base.is_or_contains_template_parameter())
      .is_some() {
      return Err(format!("found remaining template parameters: {}",
                         new_method.short_text()));
    } else {
      if let Some(conversion_type) = conversion_type {
        new_method.name = format!("operator {}", try!(conversion_type.to_cpp_code(None)));
      }
      log::noisy(format!("success: {}", new_method.short_text()));
      new_methods.push(new_method);
    }
  }
  Ok(new_methods)
}

impl CppTypeData {
  /// Checks if the type is a class type.
  pub fn is_class(&self) -> bool {
    match self.kind {
      CppTypeKind::Class { .. } => true,
      _ => false,
    }
  }

  /// Creates CppTypeBase object representing type
  /// of an object of this type. See
  /// default_template_parameters() documentation
  /// for details about handling template parameters.
  pub fn default_class_type(&self) -> CppTypeClassBase {
    if !self.is_class() {
      panic!("not a class");
    }
    CppTypeClassBase {
      name: self.name.clone(),
      template_arguments: self.default_template_parameters(),
    }
  }

  /// Creates template parameters expected for this type.
  /// For example, QHash<QString, int> will have 2 default
  /// template parameters with indexes 0 and 1. This function
  /// is helpful for determining type of "this" pointer.
  /// Result of this function may differ from actual template
  /// parameters, for example:
  /// - if a class is inside another template class,
  /// nested level should be 1 instead of 0;
  /// - if QList<V> type is used inside QHash<K, V> type,
  /// QList's template parameter will have index = 1
  /// instead of 0.
  pub fn default_template_parameters(&self) -> Option<Vec<CppType>> {
    match self.kind {
      CppTypeKind::Class { ref template_arguments, .. } => {
        match *template_arguments {
          None => None,
          Some(ref strings) => {
            Some(strings.iter()
              .enumerate()
              .map(|(num, _)| {
                CppType {
                  is_const: false,
                  indirection: CppTypeIndirection::None,
                  base: CppTypeBase::TemplateParameter {
                    nested_level: 0,
                    index: num as i32,
                  },
                }
              })
              .collect())
          }
        }
      }
      _ => None,
    }
  }

  /// Checks if the type was directly derived from specified type.
  #[allow(dead_code)]
  pub fn inherits(&self, class_name: &String) -> bool {
    if let CppTypeKind::Class { ref bases, .. } = self.kind {
      for base in bases {
        if let CppTypeBase::Class(CppTypeClassBase { ref name, .. }) = base.base {
          if name == class_name {
            return true;
          }
        }
      }
    }
    false
  }
}

impl CppData {
  /// Adds destructors for every class that does not have explicitly
  /// defined destructor, allowing to create wrappings for
  /// destructors implicitly available in C++.
  pub fn ensure_explicit_destructors(&mut self) {
    for type1 in &self.types {
      if let CppTypeKind::Class { .. } = type1.kind {
        let class_name = &type1.name;
        let mut found_destructor = false;
        for method in &self.methods {
          if method.is_destructor() && method.class_name() == Some(class_name) {
            found_destructor = true;
            break;
          }
        }
        if !found_destructor {
          let is_virtual = self.has_virtual_destructor(class_name);
          self.methods.push(CppMethod {
            name: format!("~{}", class_name),
            class_membership: Some(CppMethodClassMembership {
              class_type: type1.default_class_type(),
              is_virtual: is_virtual,
              is_pure_virtual: false,
              is_const: false,
              is_static: false,
              visibility: CppVisibility::Public,
              is_signal: false,
              kind: CppMethodKind::Destructor,
            }),
            operator: None,
            return_type: CppType::void(),
            arguments: vec![],
            allows_variadic_arguments: false,
            include_file: type1.include_file.clone(),
            origin_location: None,
            template_arguments: None,
          });
        }
      }
    }
  }

  /// Helper function that performs a portion of add_inherited_methods implementation.
  fn add_inherited_methods_from(&mut self, base_name: &String) {
    // TODO: speed up this method
    let mut new_methods = Vec::new();
    let mut derived_types = Vec::new();
    {
      for type1 in &self.types {
        if let CppTypeKind::Class { ref bases, .. } = type1.kind {
          for base in bases {
            if let CppTypeBase::Class(CppTypeClassBase { ref name, ref template_arguments }) =
                   base.base {
              if name == base_name {
                log::noisy(format!("Adding inherited methods_from {} to {}",
                                   base_name,
                                   type1.name));
                let derived_name = &type1.name;
                let base_template_arguments = template_arguments;
                let base_methods: Vec<_> = self.methods
                  .iter()
                  .filter(|method| {
                    if let Some(ref info) = method.class_membership {
                      &info.class_type.name == base_name &&
                      &info.class_type.template_arguments == base_template_arguments &&
                      !info.kind.is_constructor() &&
                      !info.kind.is_destructor() &&
                      method.operator != Some(CppOperator::Assignment)
                    } else {
                      false
                    }
                  })
                  .collect();
                derived_types.push(derived_name.clone());
                for base_class_method in base_methods.clone() {
                  let mut ok = true;
                  for method in &self.methods {
                    if method.class_name() == Some(derived_name) &&
                       method.name == base_class_method.name {
                      log::noisy("Method is not added because it's overriden in derived class");
                      log::noisy(format!("Base method: {}", base_class_method.short_text()));
                      log::noisy(format!("Derived method: {}\n", method.short_text()));
                      ok = false;
                      break;
                    }
                  }
                  if ok {
                    let mut new_method = base_class_method.clone();
                    if let Some(ref mut info) = new_method.class_membership {
                      info.class_type = type1.default_class_type();
                    } else {
                      panic!("class_membership must be present");
                    }
                    new_method.include_file = type1.include_file.clone();
                    new_method.origin_location = None;
                    log::noisy(format!("Method added: {}", new_method.short_text()));
                    log::noisy(format!("Base method: {} ({:?})\n",
                                       base_class_method.short_text(),
                                       base_class_method.origin_location));
                    new_methods.push(new_method.clone());
                  }
                }
              }
            }
          }
        }
      }
    }
    self.methods.append(&mut new_methods);
    for name in derived_types {
      self.add_inherited_methods_from(&name);
    }
  }

  /// Adds methods of derived classes inherited from base classes.
  /// A method will not be added if there is a method with the same
  /// name in the derived class. Constructors, destructors and assignment
  /// operators are also not added. This reflects C++'s method inheritance rules.
  pub fn add_inherited_methods(&mut self) {
    log::info("Adding inherited methods");
    let all_type_names: Vec<_> = self.types.iter().map(|t| t.name.clone()).collect();
    for name in all_type_names {
      self.add_inherited_methods_from(&name);
    }
    log::info("Finished adding inherited methods");
  }

  /// Generates duplicate methods with fewer arguments for
  /// C++ methods with default argument values.
  pub fn generate_methods_with_omitted_args(&mut self) {
    let mut new_methods = Vec::new();
    for method in &self.methods {
      if method.arguments.len() > 0 && method.arguments.last().unwrap().has_default_value {
        let mut method_copy = method.clone();
        while method_copy.arguments.len() > 0 &&
              method_copy.arguments.last().unwrap().has_default_value {
          method_copy.arguments.pop().unwrap();
          new_methods.push(method_copy.clone());
        }
      }
    }
    self.methods.append(&mut new_methods);
  }

  pub fn all_include_files(&self) -> HashSet<String> {
    let mut result = HashSet::new();
    for method in &self.methods {
      if !result.contains(&method.include_file) {
        result.insert(method.include_file.clone());
      }
    }
    for tp in &self.types {
      if !result.contains(&tp.include_file) {
        result.insert(tp.include_file.clone());
      }
    }
    result
  }

  /// Checks if specified class is a template class.
  #[allow(dead_code)]
  pub fn is_template_class(&self, name: &String) -> bool {
    if let Some(type_info) = self.types.iter().find(|t| &t.name == name) {
      if let CppTypeKind::Class { ref template_arguments, ref bases, .. } = type_info.kind {
        if template_arguments.is_some() {
          return true;
        }
        for base in bases {
          if let CppTypeBase::Class(CppTypeClassBase { ref name, ref template_arguments }) =
                 base.base {
            if template_arguments.is_some() {
              return true;
            }
            if self.is_template_class(name) {
              return true;
            }
          }
        }
      }
    } else {
      log::warning(format!("Unknown type assumed to be non-template: {}", name));
    }
    false
  }

  /// Checks if specified class has virtual destructor (own or inherited).
  pub fn has_virtual_destructor(&self, class_name: &String) -> bool {
    for method in &self.methods {
      if method.is_destructor() && method.class_name() == Some(class_name) {
        return method.class_membership.as_ref().unwrap().is_virtual;
      }
    }
    if let Some(type_info) = self.types.iter().find(|t| &t.name == class_name) {
      if let CppTypeKind::Class { ref bases, .. } = type_info.kind {
        for base in bases {
          if let CppTypeBase::Class(CppTypeClassBase { ref name, .. }) = base.base {
            if self.has_virtual_destructor(name) {
              return true;
            }
          }
        }
      }
    }
    return false;
  }


  #[allow(dead_code)]
  pub fn get_all_methods(&self, class_name: &String) -> Vec<&CppMethod> {
    let own_methods: Vec<_> = self.methods
      .iter()
      .filter(|m| m.class_name() == Some(class_name))
      .collect();
    let mut inherited_methods = Vec::new();
    if let Some(type_info) = self.types.iter().find(|t| &t.name == class_name) {
      if let CppTypeKind::Class { ref bases, .. } = type_info.kind {
        for base in bases {
          if let CppTypeBase::Class(CppTypeClassBase { ref name, .. }) = base.base {
            for method in self.get_all_methods(name) {
              if own_methods.iter()
                .find(|m| m.name == method.name && m.argument_types_equal(&method))
                .is_none() {
                inherited_methods.push(method);
              }
            }
          }
        }
      } else {
        panic!("get_all_methods: not a class");
      }
    } else {
      log::warning(format!("get_all_methods: no type info for {:?}", class_name));
    }
    for method in own_methods {
      inherited_methods.push(method);
    }
    inherited_methods
  }

  pub fn get_pure_virtual_methods(&self, class_name: &String) -> Vec<&CppMethod> {

    let own_methods: Vec<_> = self.methods
      .iter()
      .filter(|m| m.class_name() == Some(class_name))
      .collect();
    let own_pure_virtual_methods: Vec<_> = own_methods.iter()
      .filter(|m| {
        m.class_membership
          .as_ref()
          .unwrap()
          .is_pure_virtual
      })
      .collect();
    let mut inherited_methods = Vec::new();
    if let Some(type_info) = self.types.iter().find(|t| &t.name == class_name) {
      if let CppTypeKind::Class { ref bases, .. } = type_info.kind {
        for base in bases {
          if let CppTypeBase::Class(CppTypeClassBase { ref name, .. }) = base.base {
            for method in self.get_pure_virtual_methods(name) {
              if own_methods.iter()
                .find(|m| m.name == method.name && m.argument_types_equal(&method))
                .is_none() {
                inherited_methods.push(method);
              }
            }
          }
        }
      } else {
        panic!("get_pure_virtual_methods: not a class");
      }
    } else {
      log::warning(format!("get_pure_virtual_methods: no type info for {:?}",
                           class_name));
    }
    for method in own_pure_virtual_methods {
      inherited_methods.push(method);
    }
    inherited_methods
  }



  fn instantiate_templates(&mut self) {
    log::info("Instantiating templates.");
    let mut new_methods = Vec::new();
    for method in &self.methods {
      for type1 in method.all_involved_types() {
        if let CppTypeBase::Class(CppTypeClassBase { ref name, ref template_arguments }) =
               type1.base {
          if let &Some(ref template_arguments) = template_arguments {
            assert!(!template_arguments.is_empty());
            if template_arguments.iter().find(|x| !x.base.is_template_parameter()).is_none() {
              if self.template_instantiations.contains_key(name) {
                let nested_level = if let CppTypeBase::TemplateParameter { nested_level, .. } =
                                          template_arguments[0].base {
                  nested_level
                } else {
                  panic!("only template parameters can be here");
                };
                log::noisy(format!(""));
                log::noisy(format!("method: {}", method.short_text()));
                log::noisy(format!("found template class: {}", name));
                match apply_instantiations_to_method(method,
                                                     nested_level,
                                                     &self.template_instantiations[name]) {
                  Ok(mut methods) => {
                    new_methods.append(&mut methods);
                    break;
                  }
                  Err(msg) => log::noisy(format!("failed: {}", msg)),
                }
                break;
              }
            }
          }
        }
      }
    }
    self.methods.append(&mut new_methods);
  }


  /// Performs data conversion to make it more suitable
  /// for further wrapper generation.
  pub fn post_process(&mut self) {
    self.ensure_explicit_destructors();
    self.generate_methods_with_omitted_args();
    self.instantiate_templates();
    self.add_inherited_methods();
  }
}
