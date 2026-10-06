These are authored, minimal Quicker-format fixtures for parser and interpreter
regression tests. They replace references to absent `sample/` exports from the
prototype; they are not copies of those original exports. The formula image
workflow uses mocked dialogs, downloads, and clipboard operations in unit tests.

Coverage includes legacy launch and key macro documents, nested conditions,
variable bindings, clipboard formats, regex extraction, image conversion, and
action state. These tests do not prove compatibility with every Quicker action.
