import SwiftUI

/// Type a repeater's router hint by hand: the start of its address, or the
/// hint itself in hex.
///
/// The form is chosen rather than guessed, since text such as `a1b2` reads
/// in both alphabets. Four base58 characters can straddle two hints; when
/// they do, both are offered and one must be picked.
struct RouterHintEntryScreen: View {
    let actions: PeerActions
    let isInPath: (MeshRouterHint) -> Bool
    let add: (MeshRouterHint) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var format = RouterHintFormat.base58Prefix
    @State private var text = ""
    @State private var input = RouterHintInput.empty
    @State private var picked: MeshRouterHint?
    @FocusState private var isFieldFocused: Bool

    private struct ParseKey: Hashable {
        let text: String
        let format: RouterHintFormat
    }

    var body: some View {
        List {
            Section {
                Picker("Form", selection: $format) {
                    Text("Base58 Prefix").tag(RouterHintFormat.base58Prefix)
                    Text("Hex").tag(RouterHintFormat.hex)
                }
                .pickerStyle(.segmented)
                TextField(format == .hex ? "5EBD" : "7Nq4", text: $text)
                    .font(.body.monospaced())
                    .keyboardType(.asciiCapable)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .focused($isFieldFocused)
                    .submitLabel(.done)
                    .onSubmit(addChosen)
            } footer: {
                Text(fieldFooter)
            }

            if input.candidates.count == 1, let hint = input.candidates.first {
                Section("Hop") {
                    LabeledContent("Router hint", value: hint.text)
                    LabeledContent("Hex", value: hint.hexText)
                    LabeledContent(
                        "Known node",
                        value: RouterHintNaming.match(hint, among: actions.knownPeers)?.displayName ?? "None"
                    )
                    if isInPath(hint) {
                        Text("Already in the path.").foregroundStyle(.secondary)
                    }
                }
            } else if input.candidates.count > 1 {
                Section {
                    ForEach(input.candidates, id: \.self) { hint in
                        Button {
                            picked = hint
                        } label: {
                            candidateRow(hint)
                        }
                        .buttonStyle(.plain)
                        .disabled(isInPath(hint))
                    }
                } header: {
                    Text("Two hints fit")
                } footer: {
                    Text("Type another character, or pick one.")
                }
            }
        }
        .navigationTitle("Router Hint")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .confirmationAction) {
                Button("Add", action: addChosen)
                    .disabled(chosen == nil)
            }
        }
        .onAppear { isFieldFocused = true }
        .task(id: ParseKey(text: text, format: format)) {
            picked = nil
            let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty, let parse = actions.parseRouterHint else {
                input = .empty
                return
            }
            input = await parse(trimmed, format)
        }
    }

    private func candidateRow(_ hint: MeshRouterHint) -> some View {
        let named = RouterHintNaming.match(hint, among: actions.knownPeers)
        return PeerRow(
            hint: named?.identity.hint ?? MeshNodeHint(bytes: hint.bytes, text: hint.text),
            shape: named?.avatarShape ?? .hexagon,
            title: named?.displayName ?? "Unknown node",
            subtitle: "Router hint \(hint.text) · \(hint.hexText)",
            size: .compact,
            trailing: {
                if picked == hint {
                    Image(systemName: "checkmark").foregroundStyle(.tint)
                } else if isInPath(hint) {
                    Text("In path").foregroundStyle(.secondary)
                }
            }
        )
    }

    /// The one hint Add would append, once there is exactly one.
    private var chosen: MeshRouterHint? {
        let hint = input.candidates.count == 1 ? input.candidates.first : picked
        guard let hint, !isInPath(hint) else { return nil }
        return hint
    }

    private func addChosen() {
        guard let chosen else { return }
        add(chosen)
        dismiss()
    }

    private var fieldFooter: String {
        let isEmpty = text.trimmingCharacters(in: .whitespaces).isEmpty
        switch (format, isEmpty ? nil : input.problem) {
        case (.base58Prefix, .invalidCharacter?):
            return "Base58 has no 0, O, I, or l."
        case (.base58Prefix, .outOfRange?):
            return "No address starts this way."
        case (.base58Prefix, .tooLong?):
            return "Longer than an address."
        case (.base58Prefix, _):
            return "The first 4 or more characters of the address."
        case (.hex, .invalidCharacter?):
            return "Only 0–9 and A–F."
        case (.hex, _):
            return "Exactly 4 hex digits."
        }
    }
}
