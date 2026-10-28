#!/usr/bin/env python3
"""Actual offline format extraction through unchanged HTLK + mcp-sandbox.

Docker CLI is used only for fixture generation/image metadata. All MCP operations
and production native processing are owned by compiled HTLK operator workflows.
No model calls. Evidence is written under ignored target/providers.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import tarfile
import tempfile
import uuid
import zipfile
from live_provider_spike import ROOT, execute, host, write_json


def fixtures(directory, image):
    sources = {}
    def add(name, media, data):
        (directory / name).write_bytes(data.encode() if isinstance(data, str) else data)
        sources[name] = media
    for name, media, value in [
        ("plain.txt", "text/plain", "The ocean is blue."),
        ("notes.md", "text/markdown", "# Ocean\nThe ocean is blue."),
        ("page.html", "text/html", '<!doctype html><html><body><p>The ocean is blue.</p><script>UNTRUSTED_SCRIPT</script><img src="https://invalid.example/never-fetch"></body></html>'),
        ("table.csv", "text/csv", 'subject,color\nocean,blue\n'),
        ("data.json", "application/json", '{"ocean":"blue"}'),
        ("code.rs", "text/x-rust", 'fn ocean() -> &\'static str { "blue" }'),
    ]: add(name, media, value)
    stream = b"BT /F1 18 Tf 40 100 Td (The ocean is blue.) Tj ET"
    objects = [b"<< /Type /Catalog /Pages 2 0 R >>", b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
               b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
               b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>", b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream"]
    pdf = b"%PDF-1.4\n"; offsets = [0]
    for i, obj in enumerate(objects, 1):
        offsets.append(len(pdf)); pdf += f"{i} 0 obj\n".encode() + obj + b"\nendobj\n"
    xref = len(pdf); pdf += b"xref\n0 6\n0000000000 65535 f \n"
    pdf += b"".join(f"{p:010d} 00000 n \n".encode() for p in offsets[1:])
    pdf += f"trailer\n<< /Root 1 0 R /Size 6 >>\nstartxref\n{xref}\n%%EOF\n".encode()
    add("paper.pdf", "application/pdf", pdf)
    for name, media, members in [
        ("document.docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document", {"word/document.xml":'<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>The ocean is blue.</w:t></w:r></w:p></w:body></w:document>'}),
        ("slides.pptx", "application/vnd.openxmlformats-officedocument.presentationml.presentation", {"ppt/slides/slide1.xml":'<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><a:p><a:r><a:t>The ocean is blue.</a:t></a:r></a:p></p:cSld></p:sld>'}),
        ("sheet.xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", {"xl/sharedStrings.xml":'<sst><si><t>ocean</t></si><si><r><t>bl</t></r><r><t>ue</t></r></si></sst>', "xl/worksheets/sheet1.xml":'<worksheet><sheetData><row><c t="s"><v>0</v></c><c t="s"><v>1</v></c><c><v>42</v></c></row></sheetData></worksheet>'}),
        ("book.epub", "application/epub+zip", {"mimetype":"application/epub+zip", "OEBPS/page.xhtml":'<html xmlns="http://www.w3.org/1999/xhtml"><body><p>The ocean is blue.</p></body></html>'}),
        ("repository.zip", "application/zip", {"src/main.rs":'fn main() { println!("The ocean is blue."); }', "README.md":"The ocean is blue."}),
    ]:
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for path, value in members.items(): archive.writestr(path, value)
        add(name, media, buffer.getvalue())
    for suffix, mode, media in [("tar", "w", "application/x-tar"), ("tar.gz", "w:gz", "application/gzip")]:
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode=mode) as archive:
            value = b"The ocean is blue."; info = tarfile.TarInfo("src/readme.txt"); info.size = len(value); archive.addfile(info, io.BytesIO(value))
        add("repository." + suffix, media, buffer.getvalue())
    def ffmpeg(arguments):
        execute(["docker", "run", "--rm", "--network", "none", "--user", f"{os.getuid()}:{os.getgid()}", "--mount", f"type=bind,src={directory},dst=/work", "--workdir", "/work", image, "ffmpeg", "-y", "-v", "error", *arguments], capture=True)
    for extension, media in [("jpg","image/jpeg"),("png","image/png"),("webp","image/webp"),("gif","image/gif"),("tiff","image/tiff")]:
        ffmpeg(["-f","lavfi","-i","color=c=blue:s=64x64:r=1","-frames:v","1","-threads","1","blue."+extension]); sources["blue."+extension]=media
    for extension, media in [("mp3","audio/mpeg"),("wav","audio/wav"),("m4a","audio/mp4"),("flac","audio/flac"),("ogg","audio/ogg")]:
        ffmpeg(["-f","lavfi","-i","flite=text=The ocean is blue.:voice=slt","-t","3","speech."+extension]); sources["speech."+extension]=media
    for extension, media, codecs in [("mp4","video/mp4",["-c:v","mpeg4","-c:a","aac"]),("mov","video/quicktime",["-c:v","mpeg4","-c:a","aac"]),("webm","video/webm",["-c:v","libvpx","-c:a","libvorbis"]),("mkv","video/x-matroska",["-c:v","mpeg4","-c:a","aac"])]:
        ffmpeg(["-f","lavfi","-i","color=c=blue:s=64x64:r=1","-f","lavfi","-i","flite=text=The ocean is blue.:voice=slt","-t","3",*codecs,"-threads","1","video."+extension]); sources["video."+extension]=media
    return sources


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument("--image",default="context-mcp-media:processing");args=parser.parse_args()
    directory=Path(tempfile.mkdtemp(prefix="formats-",dir=ROOT/"target/providers"));os.chmod(directory,0o700)
    source=directory/"fixtures";source.mkdir();profiles=directory/"profiles";profiles.mkdir()
    image=execute(["docker","image","inspect","--format","{{.Id}}",args.image],capture=True).stdout.strip()
    sources=fixtures(source,image)
    (profiles/"context.yml").write_text("apiVersion: docker.profiles.sandbox.teran.dev/v1\nname: context\ndescription: Offline bounded source processing\nspec:\n"
        +f"  image: {image}\n  network:\n    mode: none\n  cgroups:\n    cpu_cores: 1\n    memory_gb: 2\n    pids_limit: 128\n  user:\n    uid: {os.getuid()}\n    gid: {os.getgid()}\n  sudo: never\n  cwd:\n    mode: rw\n  shell: bash\n")
    env=os.environ.copy();env.update(ALLOW_DIRS=str(directory),SANDBOX_PROFILES_DIR=str(profiles),DOCKER_HOST=execute(["docker","context","inspect","--format","{{.Endpoints.docker.Host}}"],capture=True).stdout.strip())
    endpoints={"initial-sandbox-provider":{"transport":"stdio","program":str(ROOT/"target/providers/mcp-sandbox"),"arguments":[],"environment":{},"inherit_environment":["ALLOW_DIRS","SANDBOX_PROFILES_DIR","DOCKER_HOST"]}}
    selection=json.loads((ROOT/"examples/integration/discover.json").read_text());del selection["servers"]["llm"];selection["endpoints"]=endpoints
    write_json(directory/"discovery.json",selection);catalogs=directory/"catalogs.json";host("discover",directory/"discovery.json",catalogs,env)
    evidence=[]
    for name,media in sources.items():
        job=directory/("job-"+name);job.mkdir();shutil.copyfile(source/name,job/"source.input")
        request={"job":str(uuid.uuid4()),"asset":{"media_type":media},"directory":str(job),"cursor":None}
        write_json(job/"request.json",request)
        config={"source":str(ROOT/"examples/operator/extract.htlk"),"catalogs":str(catalogs),"inputs":{"request":request},"endpoints":endpoints,"database":"runtime.sqlite","idempotency_key":name}
        write_json(job/"config.json",config);result=host("run",job/"config.json",job/"result.json",env)
        accepted=result["outputs"]["result"];assert accepted["native_exit"]==0 and accepted["destroyed"]==1
        extraction=json.loads(accepted["document"]["value"]);assert extraction["media_type"]==media and extraction["segments"]
        segments=extraction["segments"]
        if media.startswith("image/"):assert any(s["kind"]=="image" for s in segments)
        elif media.startswith("audio/"):assert any(s["kind"]=="audio" for s in segments)
        elif media.startswith("video/"):assert {s["kind"] for s in segments}=={"image","audio"}
        else:
            combined=" ".join(s.get("text","") for s in segments).lower();assert "ocean" in combined and "blue" in combined and "untrusted_script" not in combined
        for segment in segments:
            if "file" in segment:assert hashlib.sha256((job/segment["file"]).read_bytes()).hexdigest()==segment["sha256"]
        evidence.append({"file":name,"media_type":media,"segments":len(segments),"fingerprint":result["fingerprint"]})
        print("Verified",name)
    write_json(directory/"evidence.json",{"image":image,"cases":evidence,"provider":"actual mcp-sandbox through HTLK","models_called":False})
    print("Format acceptance passed:",directory.relative_to(ROOT))


if __name__=="__main__":main()
