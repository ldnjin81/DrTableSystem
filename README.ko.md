# DrTableSystem

**DesignToRuntime Table System** — 기획자가 엑셀에 적은 데이터를 게임 런타임까지 그대로 옮기는 테이블 시스템.

[English](README.md)

- **스키마와 데이터 분리.** 테이블·열거형 구조는 프로그래머가 관리하는 스키마 파일(`.schema.xlsx` 또는 `.schema.yaml`)에, 데이터는 기획자의 엑셀에 둡니다. 코드는 스키마에서만 생성하므로 데이터를 고쳐도 코드는 바뀌지 않습니다.
- **언리얼 C++**: `USTRUCT` 행, `UENUM` 열거형, 테이블마다 DataAsset 클래스, 타입 안전 조회(`Find`, `FindBy<서브키>`, `Get<참조>`).
- **클라·서버 JSON**을 같은 데이터에서 필드 범위별로 나눠 만듭니다.
- **언리얼 플러그인**: 클라 JSON을 키 인덱스까지 계산된 DataAsset으로 굽고, 복사 없이 읽으며, 오래된 에셋을 거부합니다.
- **테이블 간 참조**(`Ref<Items>`, `Ref<DropTable.GroupId>`)를 `drtable check`로 검사하고 Mermaid 그래프로 그립니다.
- 한 테이블을 여러 시트·파일로 나누기, 결정적 산출물, 영어·한국어 메시지.

```
Schemas/*.schema.xlsx|yaml ─┐                 ┌─▶ C++ 헤더 ─▶ 컴파일
Schemas/Enums/*.enum.*     ─┼▶ drtable build ─┼─▶ 클라 JSON ─▶ DrTableBake ─▶ DataAsset ─▶ 런타임 조회
데이터 엑셀(*.xlsx)         ─┘                 └─▶ 서버 JSON ─▶ 서버
```

## 빠른 시작

```sh
uv sync
uv run drtable --lang ko build --input Design/Tables --schema Design/Tables/Schemas --ue-plugin --prefix Gm \
  --out-cpp Source/MyGame/TableData/Generated --out-client Intermediate/DrTable/client --out-server Build/ServerData
uv run drtable --lang ko check --client Intermediate/DrTable/client --server Build/ServerData
uv run drtable --lang ko headers --input Design/Tables --schema Design/Tables/Schemas   # 2·3행에 스키마 표시
```

테이블 스키마(`Design/Tables/Schemas/Items.schema.yaml`):

```yaml
table: Items
fields:
  - {name: Id,     type: ID<int32>,     scope: all}
  - {name: Kind,   type: SubKey<EItemType>, scope: all}
  - {name: Price,  type: int32=0,       scope: server}
```

데이터 엑셀은 1행에 필드명(열 순서 자유), 4행부터 데이터를 적습니다. 시트 이름이 테이블 이름입니다.

## 문서

- 매뉴얼: [한국어](docs/ko/manual.md) · [English](docs/en/manual.md)
- 언리얼 플러그인: `unreal/DrTableSystem` (Unreal Engine 5.8)

필요한 것: Python 3.12 이상, `openpyxl`, `pyyaml`.
