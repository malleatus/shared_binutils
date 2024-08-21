---  Configuration for the application.
---@class Config
---  Optional tmux configuration. Including sessions and windows to be created.
---@field tmux Tmux|nil
---  Optional configuration for cache-shell-setup
---@field shell_caching ShellCache|nil
---  Optional list of crate locations (used as a lookup path for tmux windows `linked_crates`)
---@field crate_locations string[]|nil

---@class ShellCache
---@field source string
---@field destination string

---  Tmux configuration.
---@class Tmux
---  List of tmux sessions.
---@field sessions Session[]
---  The default session to attach to when `startup-tmux --attach` is ran.
---@field default_session string|nil

---  Configuration for a tmux session.
---@class Session
---  Name of the session.
---@field name string
---  List of windows in the session.
---@field windows Window[]

---@alias Command string|string[]

---  Configuration for a tmux window.
---@class Window
---  Name of the window.
---@field name string
---  Optional path to set as the working directory for the window.
---@field path string|nil
---  Optional command to run in the window.
---@field command Command|nil
---  Additional environment variables to set in the window.
---@field env table<string, string>|nil
---  The names of any of the workspaces crates that provide binaries that should be available on  $PATH inside the new window.
---@field linked_crates string[]|nil

