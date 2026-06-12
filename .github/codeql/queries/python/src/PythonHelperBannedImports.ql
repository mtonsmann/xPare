/**
 * @name xPare Python helper banned import
 * @description xPare Python helper scripts must stay stdlib-only and capability-light, without network, process, concurrency, persistence, or dynamic import modules.
 * @kind problem
 * @problem.severity error
 * @security-severity 6.5
 * @precision high
 * @id xpare/python-helper-banned-import
 * @tags security
 *       maintainability
 */

import python

private predicate inPythonHelper(AstNode node) {
  node.getLocation().getFile().getRelativePath().matches("shells/macos/%.py")
}

private predicate bannedImportRoot(string root, string reason) {
  root = "asyncio" and reason = "async scheduling capability"
  or
  root = "ctypes" and reason = "native-code loading capability"
  or
  root = "http" and reason = "network capability"
  or
  root = "importlib" and reason = "dynamic import capability"
  or
  root = "multiprocessing" and reason = "process creation capability"
  or
  root = "os" and reason = "process, environment, and filesystem capability"
  or
  root = "pickle" and reason = "dynamic deserialization capability"
  or
  root = "requests" and reason = "network capability"
  or
  root = "runpy" and reason = "dynamic code execution capability"
  or
  root = "shelve" and reason = "persistence capability"
  or
  root = "socket" and reason = "network capability"
  or
  root = "subprocess" and reason = "process execution capability"
  or
  root = "sys" and reason = "process/environment capability"
  or
  root = "urllib" and reason = "network capability"
}

bindingset[moduleName, root]
private predicate moduleIsOrIsUnder(string moduleName, string root) {
  moduleName = root
  or
  moduleName.matches(root + ".%")
}

from ImportingStmt importStmt, string moduleName, string root, string reason
where
  inPythonHelper(importStmt) and
  moduleName = importStmt.getAnImportedModuleName() and
  bannedImportRoot(root, reason) and
  moduleIsOrIsUnder(moduleName, root)
select importStmt,
  "xPare Python helpers must remain capability-light; imported module '" + moduleName +
    "' adds " + reason + "."
