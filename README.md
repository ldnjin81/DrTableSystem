# datagen

엑셀 한 벌에서 **서버 JSON · 클라이언트 JSON · 언리얼 C++ 코드**를 뽑는 데이터 생성기.

기획자가 엑셀에서 데이터를 관리하고, 생성기가 서버와 클라이언트가 쓸 산출물을 한 번에 만든다. 행 구조체와 열거형 코드도 생성하므로 스키마가 한 곳(엑셀)에만 존재한다.

```
Tables.xlsx ──┬─→ C++ (USTRUCT 행 구조체, UENUM 열거형, 테이블 목록)
              ├─→ 클라이언트 JSON (클라 범위 필드만)
              └─→ 서버 JSON (서버 범위 필드만)
```

## 사용

```powershell
uv sync
uv run datagen build --input Data\Tables.xlsx --out-cpp Generated --out-client Data\client --out-server Data\server
uv run datagen check --input Data\Tables.xlsx
```

## 엑셀 규약 요약

- 시트 이름 `T_*`는 테이블, `E_*`는 열거형. 열거형을 먼저 생성한다.
- 테이블 헤더는 3행: **필드명 / 자료형 / 범위**. 데이터는 4행부터.
- 키는 자료형을 감싸서 적는다: `ID<int32>`(기본키, 테이블마다 하나), `SubKey<FName>`(서브키, 여러 개 가능). 일반 필드는 자료형만 쓴다.
- 범위는 `B`(둘 다) · `C`(클라만) · `S`(서버만) · `#`(주석, 제외).
- `Reward[0]`, `Reward[1]` 처럼 번호를 붙인 열은 하나의 고정 배열 필드로 묶인다(`int32 Reward[N];`, JSON은 배열).

자세한 규칙과 산출물 형식은 [docs/SPEC.md](docs/SPEC.md)에 있다.

- Codex 작업 규칙: [AGENTS.md](AGENTS.md)
- 설치 위치: PC `C:\tools\datagen`, 맥 `~/Project/datagen`
- 상태: 명세 확정, 구현 대기
