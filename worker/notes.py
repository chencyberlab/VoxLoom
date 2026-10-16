"""Plain meeting-note cleanup shared by generation and Word export."""
import re


def clean_meeting_notes(text):
    text = re.sub(r"\[\d{2}:\d{2}:\d{2}\]", "", text)
    text = re.sub(r"(?m)^(\s*(?:[-*]\s*)?)\d{2}:\d{2}:\d{2}\s*[-–—:]?\s*", r"\1", text)
    return "\n".join(re.sub(r"[ \t]{2,}", " ", line).rstrip() for line in text.splitlines()).strip()


MEETING_SECTIONS = ("Overview", "Key discussion points", "Discussion", "Decisions", "Action items", "Next steps", "Open questions")


def heading_text(line, headings=()):
    """The section title on this line, or None. Saved notes mark titles with
    "## "; older notes used plain titles, which are recognised by name."""
    marked = re.fullmatch(r"#{1,4}\s+(.+)", line) or re.fullmatch(r"\*\*([^*]+)\*\*:?", line)
    if marked:
        return marked[1].rstrip(":").strip()
    plain = line.rstrip(":").strip()
    if plain.lower() in {h.lower() for h in (*MEETING_SECTIONS, *headings)}:
        return plain
    # Custom instructions ask for each title on its own line ending with a colon.
    if line.endswith(":") and re.fullmatch(r"[A-Z][\w ,&/'()-]{1,48}", plain) and len(plain.split()) <= 5:
        return plain
    return None


def blocks(text, headings=()):
    paragraph, items = [], []
    def flush():
        result=[]
        if paragraph:
            result.append(("paragraph", " ".join(paragraph)))
            paragraph.clear()
        result.extend(("bullet", item) for item in items)
        items.clear()
        return result
    for raw in text.splitlines():
        line=raw.strip()
        if not line:
            yield from flush()
            continue
        heading=heading_text(line, headings)
        if heading:
            yield from flush()
            yield "heading",heading
            continue
        # "Main concerns: The client ..." – a known title with its text on the same line.
        inline=re.fullmatch(r"(?:\*\*)?([^:*]{3,40}?)(?:\*\*)?:(?:\*\*)?\s+(.+)",line)
        if inline and inline[1].strip().lower() in {h.lower() for h in (*MEETING_SECTIONS, *headings)}:
            yield from flush()
            yield "heading",inline[1].strip()
            line=inline[2]
        bullet=re.fullmatch(r"(?:[-*•]|\d+[.)])\s+(.+)",line)
        if bullet:
            if paragraph:
                yield from flush()
            items.append(bullet[1])
        else:
            if items:
                yield from flush()
            paragraph.append(line)
    yield from flush()


def inline_runs(text):
    for part in re.split(r"(\*\*[^*]+\*\*)",text):
        if part:
            bold=part.startswith("**") and part.endswith("**")
            yield (part[2:-2] if bold else part),bold


# A section that only says it has nothing in it ("No agreed next steps were mentioned in the evidence.").
EMPTY = re.compile(r"(?:none|n/?a|not (?:discussed|specified|mentioned|stated)|no (?:decisions|action items|open questions)"
                   r"|no\b[^.]*\b(?:were|was)\s+(?:mentioned|stated|discussed|agreed|identified|raised|recorded|given|provided)\b[^.]*)\.?", re.I)


def normalise_meeting_notes(text, include_questions=True, headings=()):
    sections=[('',[])]
    for kind,value in blocks(clean_meeting_notes(text), headings):
        plain=''.join(part for part,_ in inline_runs(value))
        if kind=='heading':sections.append((plain,[]))
        else:sections[-1][1].append((kind,plain))
    result=[]
    for heading,content in sections:
        if not include_questions and heading.lower()=='open questions':continue
        if not content:continue
        if all(EMPTY.fullmatch(value.strip()) for _,value in content):continue
        if heading:result.append('## '+heading)
        result.extend(('• ' if kind=='bullet' else '')+value for kind,value in content)
        result.append('')
    return '\n'.join(result).strip()
