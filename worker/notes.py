"""Plain meeting-note cleanup shared by generation and Word export."""
import re


def clean_meeting_notes(text):
    text = re.sub(r"\[\d{2}:\d{2}:\d{2}\]", "", text)
    text = re.sub(r"(?m)^(\s*(?:[-*]\s*)?)\d{2}:\d{2}:\d{2}\s*[-–—:]?\s*", r"\1", text)
    return "\n".join(re.sub(r"[ \t]{2,}", " ", line).rstrip() for line in text.splitlines()).strip()


def blocks(text):
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
        heading=(re.fullmatch(r"#{1,4}\s+(.+)",line) or re.fullmatch(r"\*\*([^*]+)\*\*:?",line)
                 or re.fullmatch(r"(Overview|Key discussion points|Discussion|Decisions|Action items|Next steps|Open questions):?",line,re.I))
        if heading:
            yield from flush()
            yield "heading",heading[1].rstrip(":")
            continue
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


def normalise_meeting_notes(text, include_questions=True):
    sections=[('',[])]
    for kind,value in blocks(clean_meeting_notes(text)):
        plain=''.join(part for part,_ in inline_runs(value))
        if kind=='heading':sections.append((plain,[]))
        else:sections[-1][1].append((kind,plain))
    result=[]
    for heading,content in sections:
        if not include_questions and heading.lower()=='open questions':continue
        if not content:continue
        if all(value.strip().rstrip('.').lower() in {'none','not discussed','not specified','no decisions','no action items','no open questions'} for _,value in content):continue
        if heading:result.append(heading)
        result.extend(('• ' if kind=='bullet' else '')+value for kind,value in content)
        result.append('')
    return '\n'.join(result).strip()
