.pragma library

// Text from files, folders and errors inside rich text (InlineMessage, StyledText labels). The
// messages that use it start with markup, so that rich text is detected and the entities show
// as characters.
function escape(text) {
    return String(text ?? "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
}
