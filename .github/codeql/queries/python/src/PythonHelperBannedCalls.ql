/**
 * @name xPare Python helper banned call
 * @description xPare Python helper scripts must not use dynamic code execution, process execution, or network-capable APIs.
 * @kind problem
 * @problem.severity error
 * @security-severity 7.0
 * @precision high
 * @id xpare/python-helper-banned-call
 * @tags security
 *       maintainability
 */

import python
import semmle.python.ApiGraphs

private predicate inPythonHelper(API::CallNode call) {
  call.getLocation().getFile().getRelativePath().matches("shells/macos/%.py")
}

private predicate bannedBuiltinCall(API::CallNode call, string name, string reason) {
  name = "eval" and reason = "dynamic code execution" and call = API::builtin("eval").getACall()
  or
  name = "exec" and reason = "dynamic code execution" and call = API::builtin("exec").getACall()
  or
  name = "compile" and reason = "dynamic code compilation" and call = API::builtin("compile").getACall()
  or
  name = "__import__" and reason = "dynamic import" and call = API::builtin("__import__").getACall()
}

private predicate bannedModuleCall(API::CallNode call, string name, string reason) {
  name = "subprocess.*" and
  reason = "process execution" and
  call = API::moduleImport("subprocess").getMember(_).getACall()
  or
  name = "os.process" and
  reason = "process execution" and
  call =
    API::moduleImport("os")
        .getMember([
            "execl", "execle", "execlp", "execlpe", "execv", "execve", "execvp", "execvpe",
            "popen", "spawnl", "spawnle", "spawnlp", "spawnlpe", "spawnv", "spawnve",
            "spawnvp", "spawnvpe", "system"
          ])
        .getACall()
  or
  name = "socket.*" and
  reason = "network access" and
  call = API::moduleImport("socket").getMember(_).getACall()
  or
  name = "urllib.*" and
  reason = "network access" and
  call = API::moduleImport("urllib").getMember(_).getACall()
  or
  name = "http.*" and
  reason = "network access" and
  call = API::moduleImport("http").getMember(_).getACall()
  or
  name = "requests.*" and
  reason = "network access" and
  call = API::moduleImport("requests").getMember(_).getACall()
  or
  name = "importlib.*" and
  reason = "dynamic import" and
  call = API::moduleImport("importlib").getMember(_).getACall()
  or
  name = "runpy.*" and
  reason = "dynamic code execution" and
  call = API::moduleImport("runpy").getMember(_).getACall()
}

private predicate bannedCall(API::CallNode call, string name, string reason) {
  bannedBuiltinCall(call, name, reason)
  or
  bannedModuleCall(call, name, reason)
}

from API::CallNode call, string name, string reason
where inPythonHelper(call) and bannedCall(call, name, reason)
select call,
  "xPare Python helpers must stay capability-light; call to " + name + " adds " +
    reason + "."
