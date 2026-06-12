/**
 * @name xPare shipped Rust capability surface
 * @description The shipped Rust core and FFI must stay filesystem-free, and shipped Rust surfaces must not grow process or network execution capability.
 * @kind problem
 * @problem.severity error
 * @security-severity 7.0
 * @precision high
 * @id xpare/rust-shipped-capability-surface
 * @tags security
 *       maintainability
 */

import rust

private predicate inCoreOrFfi(AstNode node) {
  node.getFile().getRelativePath().matches("core/src/%")
  or
  node.getFile().getRelativePath().matches("core-ffi/src/%")
}

private predicate inShippedRust(AstNode node) {
  inCoreOrFfi(node)
  or
  node.getFile().getRelativePath().matches("cli/src/%")
}

bindingset[target]
private predicate processExecutionTarget(string target) {
  target.matches("%std::process::Command%")
  or
  target.matches("%<std::process::Command>::%")
  or
  target = "std::process::exit"
}

bindingset[target]
private predicate networkTarget(string target) {
  target.matches("%std::net%")
  or
  target.matches("%std::os::unix::net%")
  or
  target.matches("%std::os::windows::net%")
}

bindingset[target]
private predicate coreFilesystemCallTarget(string target) {
  target.matches("%std::fs%")
  or
  target.matches("%std::path%")
}

bindingset[target]
private predicate coreFilesystemSourceTarget(string target) {
  target = "std::fs"
  or
  target.matches("std::fs::%")
  or
  target = "::std::fs"
  or
  target.matches("::std::fs::%")
  or
  target = "std::path"
  or
  target.matches("std::path::%")
  or
  target = "::std::path"
  or
  target.matches("::std::path::%")
}

bindingset[target]
private predicate networkSourceTarget(string target) {
  target = "std::net"
  or
  target.matches("std::net::%")
  or
  target = "::std::net"
  or
  target.matches("::std::net::%")
  or
  target = "std::os::unix::net"
  or
  target.matches("std::os::unix::net::%")
  or
  target = "::std::os::unix::net"
  or
  target.matches("::std::os::unix::net::%")
  or
  target = "std::os::windows::net"
  or
  target.matches("std::os::windows::net::%")
  or
  target = "::std::os::windows::net"
  or
  target.matches("::std::os::windows::net::%")
}

bindingset[useTree]
private predicate resolvedUseTreePath(UseTree useTree, string target) {
  target = useTree.getPath().toString()
  or
  exists(UseTree child, string prefix, string childPath |
    child = useTree.getUseTreeList().getAUseTree() and
    prefix = useTree.getPath().toString() and
    childPath = child.getPath().toString() and
    target = prefix + "::" + childPath
  )
  or
  exists(UseTree child, UseTree grandchild, string prefix, string childPath, string grandchildPath |
    child = useTree.getUseTreeList().getAUseTree() and
    grandchild = child.getUseTreeList().getAUseTree() and
    prefix = useTree.getPath().toString() and
    childPath = child.getPath().toString() and
    grandchildPath = grandchild.getPath().toString() and
    target = prefix + "::" + childPath + "::" + grandchildPath
  )
  or
  exists(
    UseTree child, UseTree grandchild, UseTree greatGrandchild, string prefix, string childPath,
    string grandchildPath, string greatGrandchildPath |
    child = useTree.getUseTreeList().getAUseTree() and
    grandchild = child.getUseTreeList().getAUseTree() and
    greatGrandchild = grandchild.getUseTreeList().getAUseTree() and
    prefix = useTree.getPath().toString() and
    childPath = child.getPath().toString() and
    grandchildPath = grandchild.getPath().toString() and
    greatGrandchildPath = greatGrandchild.getPath().toString() and
    target = prefix + "::" + childPath + "::" + grandchildPath + "::" + greatGrandchildPath
  )
}

private predicate forbiddenCallCapability(Call call, string kind, string policySurface) {
  exists(string target |
    target = call.getStaticTarget().getCanonicalPath() and
    (
      inShippedRust(call) and
      processExecutionTarget(target) and
      kind = "process execution" and
      policySurface = "core, FFI, and CLI"
      or
      inShippedRust(call) and
      networkTarget(target) and
      kind = "network access" and
      policySurface = "core, FFI, and CLI"
      or
      inCoreOrFfi(call) and
      coreFilesystemCallTarget(target) and
      kind = "filesystem or path access" and
      policySurface = "core and FFI"
    )
  )
}

private predicate forbiddenSourceCapability(AstNode node, string kind, string policySurface) {
  exists(PathTypeRepr typeRepr, string target |
    node = typeRepr and
    inCoreOrFfi(typeRepr) and
    target = typeRepr.getPath().toString() and
    coreFilesystemSourceTarget(target) and
    kind = "filesystem or path type reference" and
    policySurface = "core and FFI"
  )
  or
  exists(UseTree useTree, string target |
    node = useTree and
    inCoreOrFfi(useTree) and
    resolvedUseTreePath(useTree, target) and
    coreFilesystemSourceTarget(target) and
    kind = "filesystem or path import" and
    policySurface = "core and FFI"
  )
  or
  exists(PathTypeRepr typeRepr, string target |
    node = typeRepr and
    inShippedRust(typeRepr) and
    target = typeRepr.getPath().toString() and
    networkSourceTarget(target) and
    kind = "network type reference" and
    policySurface = "core, FFI, and CLI"
  )
  or
  exists(UseTree useTree, string target |
    node = useTree and
    inShippedRust(useTree) and
    resolvedUseTreePath(useTree, target) and
    networkSourceTarget(target) and
    kind = "network import" and
    policySurface = "core, FFI, and CLI"
  )
}

private predicate forbiddenCapability(AstNode node, string kind, string policySurface) {
  exists(Call call |
    node = call and
    forbiddenCallCapability(call, kind, policySurface)
  )
  or
  forbiddenSourceCapability(node, kind, policySurface)
}

from AstNode node, string kind, string policySurface
where forbiddenCapability(node, kind, policySurface)
select node,
  "xPare policy forbids " + kind + " in shipped Rust " + policySurface +
    " surfaces; keep OS, filesystem, process, and network integration outside the core boundary."
