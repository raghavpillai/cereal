import Foundation

/// Web addresses written out in full (`https://…`) in a line of notes: the one
/// thing in a note's text that becomes a link besides Markdown's own, so the
/// "Chat with meeting transcript" line at the foot of generated notes can be
/// tapped. The rules, and the cases every client tests (web, desktop and this),
/// are in testdata/web-addresses.json: an address is linked only when it reads
/// as where it goes.
public enum WebAddresses {
    /// The addresses in `text` as written, with the URL each one opens.
    public static func ranges(in text: String) -> [(range: Range<String.Index>, url: URL)] {
        var found: [(range: Range<String.Index>, url: URL)] = []
        var index = text.startIndex
        while index < text.endIndex {
            guard let scheme = schemeLength(text[index...]),
                  index == text.startIndex || !isWordCharacter(text[text.index(before: index)]) else {
                index = text.index(after: index)
                continue
            }
            let end = text[index...].firstIndex(where: endsAddress) ?? text.endIndex
            let address = trimmed(text[index..<end])
            let authority = address.dropFirst(scheme).prefix { !"/?#".contains($0) }
            if isPlainHost(authority), !address.contains("\\"), !address.unicodeScalars.contains(where: isHidden),
               let url = URL(string: String(address)) {
                found.append((address.startIndex..<address.endIndex, url))
                index = address.endIndex
            } else {
                index = text.index(after: index)
            }
        }
        return found
    }

    /// A line of note Markdown as an attributed string: addresses found in the text as written are links, the
    /// Markdown around them is read piece by piece, and a Markdown link opens only an address these rules allow (or mail).
    public static func markdown(_ text: String) -> AttributedString {
        var result = AttributedString()
        var at = text.startIndex
        func plain(_ piece: Substring) {
            guard !piece.isEmpty else { return }
            var part = (try? AttributedString(markdown: String(piece), options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(piece)
            for run in Array(part.runs) {
                guard let link = run.link else { continue }
                let scheme = link.scheme?.lowercased() ?? ""
                if scheme == "mailto" { continue }
                if !["http", "https"].contains(scheme) || ranges(in: link.absoluteString).first?.range != link.absoluteString.startIndex..<link.absoluteString.endIndex {
                    part[run.range].link = nil
                }
            }
            result.append(part)
        }
        for (range, url) in ranges(in: text) {
            plain(text[at..<range.lowerBound])
            var address = AttributedString(text[range])
            address.link = url
            result.append(address)
            at = range.upperBound
        }
        plain(text[at...])
        return result
    }

    private static func schemeLength(_ text: Substring) -> Int? {
        let head = text.prefix(8).lowercased()
        return head.hasPrefix("https://") ? 8 : head.hasPrefix("http://") ? 7 : nil
    }

    private static func isWordCharacter(_ character: Character) -> Bool { character.isLetter || character.isNumber }

    private static func endsAddress(_ character: Character) -> Bool {
        character.isWhitespace || character == "\u{FEFF}" || "<>\"'`".contains(character)
    }

    /// Sentence punctuation after an address is not part of it, nor a closing bracket whose opening partner is
    /// outside it, the way GitHub's autolinker reads them.
    private static func trimmed(_ candidate: Substring) -> Substring {
        var address = candidate
        while let last = address.last {
            let pair: Character? = last == ")" ? "(" : last == "]" ? "[" : last == "}" ? "{" : nil
            let unbalanced = pair.map { open in address.filter { $0 == last }.count > address.filter { $0 == open }.count } ?? false
            if ".,:;!?*_~".contains(last) || unbalanced { address = address.dropLast() } else { break }
        }
        return address
    }

    /// A plain ASCII name or address with an optional port: no user name or password, no percent sign, no letters
    /// of another script that only look like these.
    private static func isPlainHost(_ authority: Substring) -> Bool {
        authority.range(of: #"^(?:[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.?|\[[0-9A-Fa-f:.]+\])(?::[0-9]{1,5})?$"#, options: .regularExpression) != nil
    }

    /// Control and invisible formatting characters (bidirectional overrides, zero-width spaces): they can make an
    /// address show one place and open another.
    private static func isHidden(_ scalar: Unicode.Scalar) -> Bool {
        [.control, .format].contains(scalar.properties.generalCategory)
    }
}
