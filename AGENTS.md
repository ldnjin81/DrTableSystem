# DrTableSystem — Codex 작업 규칙

- 설계 기준은 `docs/ko/manual.md`(사용자 매뉴얼)다. `docs/SPEC.md`는 초기 설계 메모라 현재 규약과 다를 수 있다. 명세와 다르게 가야 하면 구현 전에 Agent Bridge `send_message(to="claude", kind="question")`로 묻는다.
- 런타임(PC): `C:\tools\_portable\uv\uv.exe`(환경변수 `UV_PYTHON_INSTALL_DIR=C:\tools\_portable\python`, `UV_CACHE_DIR=C:\tools\_portable\uv-cache`). 시스템 Python·전역 pip 설치 금지.
  - 준비: `uv sync` / 테스트: `uv run pytest -q` / 린트: `uv run ruff check .`
- 순수 Python만 쓴다. 의존성은 최소로: `openpyxl`만 쓰고, 더 필요하면 이유를 커밋 메시지에 적는다.
- 코드 주석·문서·오류 메시지는 한국어로 쓴다. 식별자는 영어.
- **생성물은 결정적이어야 한다.** 같은 입력이면 바이트까지 같아야 한다. 딕셔너리 순회 순서에 의존하지 말고, 줄바꿈은 LF, JSON은 키 순서 고정.
- 오류 메시지에는 항상 `[파일]시트!셀` 위치를 넣는다.
- 테스트 픽스처 xlsx는 저장소에 바이너리로 넣지 말고 **테스트 안에서 openpyxl로 만든다**.
- 브랜치: `auto/<task-slug>`. main 직접 커밋 금지. 원격 `ssh://git@192.168.0.24:2222/ldnjin/tablegen.git`.
  - 무인 실행에서는 커밋·push를 직접 하지 않는다(샌드박스가 `.git`과 SSH 키를 막음). 작업트리에서 수정하고 `update_task`에 `lore_branch`만 넣으면 `codex-run.ps1`이 커밋·push한다.
- 보고: 끝나면 `report-result` 스킬로 올린다(테스트 결과, 샘플 입력 대비 산출물 요약).
