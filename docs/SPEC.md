# DrTableSystem 명세 (v0.1)

엑셀 한 벌에서 **서버 JSON · 클라이언트 JSON · C++ 코드**를 뽑는 생성기. 대규모 개발에서 재사용하는 것이 목표라 프로젝트 고유 규칙을 넣지 않는다.

- 언어: Python(순수). PC는 포터블 `C:\tools\_portable\uv`와 python 3.12를 쓴다.
- 설치 위치: PC `C:\tools\drtable`, 원격 `ssh://git@192.168.0.24:2222/ldnjin/drtable.git`
- 엑셀 읽기: `openpyxl`(xlsx 전용). 매크로·수식 결과값은 `data_only=True`로 읽는다.

---

## 1. 엑셀 규약

### 1.1 시트 이름

| 형태 | 뜻 | 예 |
|---|---|---|
| `#`으로 시작 | **설명·예외 메모용**. 파싱하지 않고 완전히 무시한다 | `#사용법`, `#예외정리` |
| `<enum>이름` | **열거형 정의 시트**. 포맷은 테이블과 같다(1.4 참고) | `<enum>ItemType` → 열거형 `ItemType` |
| 그 외 전부 | **테이블 시트**. 시트 이름이 곧 테이블 이름이다 | `Effects` → 테이블 `Effects` |

**열거형 시트를 먼저 전부 수집·생성한 뒤** 테이블을 처리한다. 테이블이 참조하는 열거형이 없으면 오류다.

여기서 **추출한 이름**(테이블은 시트 이름 그대로, 열거형은 `<enum>`을 뗀 나머지)이 C++ 식별자가 된다. 따라서 추출한 이름은 영문자로 시작하고 영숫자·밑줄만 써야 하며, 어긋나면 오류다. 시트 이름 자체의 `<enum>` 표기와 `#` 접두사는 검사 대상이 아니다.

### 1.2 테이블 시트 — 헤더 3행

테이블 시트는 `#`으로 시작하지 않고 `<enum>` 형태도 아닌 모든 시트다.

| 행 | 내용 |
|---|---|
| 1행 | 필드명 (C++ 식별자로 쓸 수 있어야 한다: 영문자로 시작, 영숫자·밑줄) |
| 2행 | **자료형. 키는 자료형을 감싸는 표기로 지정한다** (아래) |
| 3행 | 범위: `B`(둘 다) · `C`(클라만) · `S`(서버만) · `#`(주석, 산출물에서 제외) |
| 4행~ | 데이터 |

2행 문법은 `<자료형>`, `<자료형>=<기본값>`, 또는 `<역할><자료형>` 이다.

```
int32             일반 필드
name              일반 필드
EElement          일반 필드(열거형)
ID<int32>         기본키. 테이블마다 정확히 1개 필요
ID<name>          기본키를 문자열로 쓸 때
SubKey<name>      서브키. 인덱스 이름은 그 열의 필드명을 쓴다
SubKey<EElement>  열거형 서브키
float=1.0         일반 필드이며 빈 셀의 기본값은 1.0
bool=true         일반 필드이며 빈 셀의 기본값은 true
string=없음       일반 필드이며 빈 셀의 기본값은 "없음"
```

- 역할 표기는 대소문자를 가리지 않는다(`id<int32>`, `subkey<name>` 모두 허용).
- 기본값은 자료형 뒤의 첫 `=` 오른쪽에 쓴다. 앞뒤 공백은 제거하며, 문자열 기본값 안의 두 번째 이후 `=`는 값의 일부다. 셀 주석은 읽지 않는다. 3행 헤더를 유지하면서 표기가 복사·코드 리뷰·버전 비교에 그대로 드러나게 하기 위해 2행 표기를 택했다.
- 기본값은 자료 셀과 같은 변환 규칙으로 검증한다. 변환할 수 없으면 **기본값을 선언한 2행 셀 주소**를 포함해 오류로 처리한다.
- `ID<>`와 `SubKey<>`에는 기본값을 지정할 수 없다. 키의 빈 셀을 기본값으로 메우면 서로 다른 행이 같은 식별자를 암묵적으로 공유할 수 있으므로, 키 누락은 반드시 자료 행의 검증 오류로 드러내야 한다.
- 기본키와 서브키에는 `int32`, `int64`, `name`, 열거형(`E*`)만 사용할 수 있다. `string`·`tag`·`path`는 런타임 비교·변환 계약과 맞지 않고, `float`·`double`은 동등 비교가 불안정하며, `bool`은 키로서 의미가 없고, `text`는 지역화 대상이라 비교 기준이 불안정하므로 금지한다.
- 서브키는 0개 이상, 개수 제한 없다. **선언하지 않으면 인덱스를 만들지 않는다.**
- **기본키의 범위는 반드시 `B`여야 한다.** 클라와 서버 양쪽 산출물이 모두 키로 행을 찾기 때문이다. `C`나 `S`로 두면 한쪽 JSON에 키가 빠져 깨진다.
- 서브키 인덱스 이름은 필드명이므로, 필드명이 중복되지 않는 한 충돌하지 않는다.
- 복합 기본키(두 열을 묶은 키)는 v0.1 범위 밖이다.

- 범위가 `#`인 열은 자료형·키 지정을 무시하고 완전히 건너뛴다.
- 빈 열(1행이 비어 있음)을 만나면 그 오른쪽은 읽지 않는다.

예시 시트 `Effects`:

| | A | B | C | D |
|---|---|---|---|---|
| **1** | `Id` | `Name` | `Element` | `Damage` |
| **2** | `ID<int32>` | `SubKey<name>` | `SubKey<EElement>` | `float` |
| **3** | `B` | `B` | `B` | `C` |
| **4** | `1001` | `Burn` | `Fire` | `12.5` |
| **5** | `1002` | `Freeze` | `Water` | `0` |

### 1.2.1 배열 열

같은 필드명에 `[번호]`를 붙인 열이 여러 개 있으면 **하나의 고정 배열 필드**로 묶는다. 엑셀에서는 평평한 열이지만, 코드와 JSON에서는 배열처럼 다룬다.

| | A | B | C | D |
|---|---|---|---|---|
| **1** | `Id` | `Reward[0]` | `Reward[1]` | `Reward[2]` |
| **2** | `ID<int32>` | `int32` | `int32` | `int32` |
| **3** | `B` | `B` | `B` | `B` |
| **4** | `1001` | `10` | `20` | `30` |

생성 결과는 **C 스타일 고정 배열**이다. 언리얼 리플렉션이 이를 지원하며(`FProperty::ArrayDim`), 힙 할당이 없고 행 안에 연속으로 들어간다.

```cpp
UPROPERTY(EditAnywhere, Category = "Dt|Effects")
int32 Reward[3] = {};
```

**배열 열에는 `BlueprintReadOnly`를 붙이지 않는다.** UE 리플렉션은 고정 배열을 다루지만(`FProperty::ArrayDim`), 블루프린트 노출은 막혀 있어 UHT가 `Static array cannot be exposed to blueprint`로 빌드를 거부한다. 배열 필드는 `EditAnywhere`만 붙여 C++과 에디터에서만 쓴다.

JSON에서는 실제 배열로 나간다: `"Reward": [10, 20, 30]`.

규칙:

- 번호는 **0부터 빈틈없이** 이어져야 한다. `[0]`, `[1]`, `[3]`처럼 비면 오류(`Effects!D1`).
- 같은 번호가 두 번 나오면 오류.
- 배열을 이루는 열은 **자료형과 범위가 모두 같아야** 한다. 다르면 오류.
- 배열 열에는 `ID<>`나 `SubKey<>`를 쓸 수 없다(키는 스칼라만).
- 열 순서는 흩어져 있어도 번호로 묶는다. 다만 산출물 순서는 번호순으로 고정한다.
- 배열 크기는 열 개수로 정해진다. 각 물리 열에 기본값을 따로 선언할 수 있다(`int32=10`, `int32=20`). 빈 셀은 해당 물리 열의 선언값을 쓰고, 선언이 없으면 자료형 기본값을 쓴다.

### 1.3 지원 자료형

| 표기 | C++ (클라) | 클라 JSON | 서버 JSON |
|---|---|---|---|
| `int32`, `int64` | `int32`, `int64` | number | number |
| `float`, `double` | `float`, `double` | number | number |
| `bool` | `bool` | true/false | true/false |
| `name` | `FName` | string | string |
| `string` | `FString` | string | string |
| `text` | `FText` | string | string |
| `tag` | `FGameplayTag` | string | string |
| `path` | `FSoftObjectPath` | string | string |
| `E<이름>` | `EDt<이름>`(생성된 열거형) | string (열거자 이름) | string |

**서버는 언리얼이 아니므로** `name`·`string`·`text`·`tag`·`path`는 서버 JSON에서 전부 **문자열**로 나간다. 숫자·불리언은 그대로 둔다.

`path`는 **언제나 타입 무관 경로**다. `TSoftObjectPtr<타입>`은 대상 타입마다 전방 선언이 필요해 생성기가 다루기에 취약하므로 쓰지 않는다 — 필요하면 런타임에 `TryLoad()`나 `TSoftObjectPtr<T>(Path)`로 바꾼다. `path<타입>` 같은 꺾쇠 표기는 받지 않는다.

**옛 표기 `FName`·`FString`은 받지 않는다.** 만나면 "이제 `name`/`string`을 쓰세요" 안내와 함께 검증 오류로 멈춘다. 키 표기도 `ID<name>` 형태를 쓴다.

- 셀이 비어 있으면 그 열에 선언한 기본값을 쓴다. 선언이 없으면 기존 자료형 기본값(0, false, 빈 문자열, 열거형의 첫 항목)을 쓴다.
- 열별 기본값은 클라이언트·서버 JSON에 똑같이 적용한다. 범위가 `C`나 `S`인 열도 자기 산출물에서 같은 규칙을 따른다.
- 숫자 칸에 숫자가 아닌 값이 있으면 오류다(시트·셀 주소를 찍는다).
- 배열은 1.2.1의 `필드명[번호]` 표기로 지원한다. **중첩 구조체**는 v0.1 범위 밖이다.

### 1.3.1 테이블 참조 `Ref<T>`

2행의 `Ref<Items>`는 같은 입력에 포함된 `Items` 테이블의 기본키를 가리킨다. `ref` 키워드는 대소문자를 가리지 않지만 테이블 이름은 정확히 일치해야 한다. `<enum>` 시트나 `#` 시트는 대상이 아니다. 부가 열을 가진 열거형에서 생성된 `이름Info` 테이블은 대상이 될 수 있다. 대상 확인은 `build` 시 스키마 해석 단계에서 한다.

참조 필드의 실제 자료형은 대상 기본키의 자료형(`int32`·`int64`·`name`·열거형)이다. 대상 키 자료형을 바꾸면 참조 필드의 C++·JSON 자료형도 함께 바뀐다. 선언 표기 `Ref<Items>`는 `schema_hash`에 들어간다. 자기 참조와 순환 참조, 배열 열, `SubKey<Ref<T>>`를 허용한다. `ID<Ref<T>>`와 `Ref<T>=값`은 금지한다.

빈 참조 셀은 없음으로 해석한다. 숫자 키 대상은 `0`, `name` 키 대상은 빈 이름(JSON의 `""`, C++의 `NAME_None`)을 쓴다. 열거형 키 대상 참조는 비울 수 없다. 생성 시 참조 값의 실제 존재 여부는 검사하지 않는다. 생성된 JSON을 `drtable check --client`로 검사한다.

`Ref<DropTable.GroupId>`는 `DropTable`의 `SubKey<>` 필드 `GroupId`를 참조한다. 값 하나가 그 서브키를 공유하는 행 묶음(1:N)을 가리킨다. 일반 필드·기본키·없는 필드는 대상으로 쓸 수 없다. 기본키는 필드명을 쓰지 않는 `Ref<DropTable>` 형식만 쓴다. 참조 필드가 나가는 범위는 대상 서브키가 나가는 범위에 포함되어야 한다. 예를 들어 대상 서브키가 `C`이면 참조 필드는 `C`여야 한다. `SubKey<Ref<T.K>>`와 배열도 허용하며 빈 셀·기본값 금지 규칙은 기본키 참조와 같다. 대상 서브키 자체가 참조이면 최종 자료형까지 연쇄로 따라간다. 각 단계의 범위 규칙을 검사하며, 자료형을 결정할 수 없는 순환은 전체 경로를 보여 주는 스키마 오류다. 값의 자기 참조·순환은 허용한다. manifest와 검사기는 직접 대상으로 선언한 `T.K`를 사용한다.

### 1.4 열거형 시트 (`<enum>이름`)

**포맷은 테이블과 같다** — 1행 필드명, 2행 자료형, 3행 범위, 4행부터 데이터. 다른 점은 시트 이름에 `<enum>`이 붙고, **기본키가 곧 열거자 이름**이라는 것뿐이다.

| | A | B | C |
|---|---|---|---|
| **1** | `Id` | `Value` | `Comment` |
| **2** | `ID<name>` | `int32` | `string` |
| **3** | `B` | `B` | `#` |
| **4** | `Weapon` | `0` | `무기류` |
| **5** | `Armor` | `1` | `방어구` |

- 기본키(`ID<name>`) 열의 값이 **열거자 이름**이다. C++ 식별자 규칙을 따라야 하고 중복은 오류다.
- `Value` 열은 열거자 값이다. 열이 없거나 셀이 비면 위에서부터 0, 1, 2…를 매긴다. 값 중복은 오류다.
- 범위가 `#`이고 이름이 `Comment`인 열은 **생성 코드의 주석**으로 들어간다. 그 밖의 `#` 열은 테이블과 똑같이 무시한다.

열 구성에 따라 산출물이 갈린다.

**(가) 기본형 — 코드만 만든다**

`Id`와 `Value`(그리고 `#` 범위 열)뿐이면 열거형 코드만 생성한다. 테이블도 JSON도 만들지 않는다. 대부분의 열거형이 여기 해당한다.

**(나) 부가 열이 있으면 — `이름Info` 테이블도 만든다**

`#`이 아닌 열이 더 있으면 열거형 코드에 더해 **`<이름>Info` 테이블**을 생성한다. 예를 들어 `<enum>ItemType`에 `DisplayName`, `MaxStack` 열이 있으면 → 열거형 `EDtItemType` + 테이블 `ItemTypeInfo`.

- 행 구조체는 `F<접두사>ItemTypeInfoRow`, 기본키 필드의 자료형은 `FName`이 아니라 **생성된 열거형**(`EDtItemType`)이다.
- `Value` 열은 Info 테이블에 넣지 않는다. 열거형이 이미 그 값을 갖고 있다.
- 나머지는 보통 테이블과 완전히 같다 — 범위(`C`/`S`/`B`), 서브키, 배열 열(1.2.1), JSON 산출 모두 그대로다.
- `<이름>Info`가 다른 시트 이름과 겹치면 오류다.

---

## 2. 산출물

`drtable build`가 세 종류를 만든다. 모두 **결정적**이어야 한다 — 같은 입력이면 바이트가 같아야 하고, 줄바꿈은 LF, JSON 키 순서는 시트 열 순서를 따른다.

### 2.1 C++ (클라이언트)

```
<out-cpp>/EDtElement.h              // 열거형 하나당 헤더 하나
<out-cpp>/DtEffectsRow.h            // 테이블 하나당 행 구조체 하나
<out-cpp>/DtEffectsTable.h          // 테이블 하나당 에셋 클래스 하나
<out-cpp>/DtGeneratedTables.h       // 전체 목록·키 정보·스키마 해시 상수
```

열거형 시트가 만든 `이름Info` 테이블(1.4 나)도 보통 테이블과 똑같이 행 구조체 헤더와 JSON을 낸다.

- 파일 머리에 `// 자동 생성됨 — 직접 수정하지 말 것. 출처: <파일>.xlsx / <시트>` 를 넣는다.
- 행 구조체는 `USTRUCT(BlueprintType)`, 필드는 `UPROPERTY(EditAnywhere, BlueprintReadOnly)`. **단 배열 필드는 `UPROPERTY(EditAnywhere)`만 붙인다**(1.2.1 참고 — UHT가 고정 배열의 블루프린트 노출을 거부한다).
- 구조체 이름 `F<접두사><테이블>Row`, 열거형 `E<접두사><이름>`. 접두사는 `--prefix`로 받으며 기본 `Dt`.
- **클라 범위(`C`,`B`) 필드만** 넣는다.
- 열거형은 `UENUM(BlueprintType)`, 기반 타입 `uint8`.

#### 2.1.1 테이블 에셋 클래스

테이블마다 `UCLASS` 에셋 클래스를 하나 낸다. 에디터 커밋릿이 이 클래스의 인스턴스를 만들어 클라 JSON의 내용을 채우고 `.uasset`으로 굽는다(3절 파이프라인).

```cpp
UCLASS(BlueprintType)
class UDtEffectsTable : public UPrimaryDataAsset
{
    GENERATED_BODY()
public:
    // 행은 연속 배열 하나로 저장한다. 기본키 오름차순으로 정렬돼 있다.
    UPROPERTY(EditAnywhere, BlueprintReadOnly, Category = "Dt|Effects")
    TArray<FDtEffectsRow> Rows;

    // 기본키 — Rows와 같은 순서(정렬됨)라 이진 탐색이 된다.
    UPROPERTY() TArray<int32> PrimaryKeys;

    // 서브키마다 CSR 인덱스 세 벌. 선언하지 않은 서브키는 아예 생기지 않는다.
    UPROPERTY() TArray<EDtElement> Element_Keys;     // 버킷 순서의 키 값
    UPROPERTY() TArray<int32>      Element_Offsets;  // 버킷 경계(크기 = Keys+1)
    UPROPERTY() TArray<int32>      Element_Indices;  // Rows 인덱스
};
```

- **인덱스는 생성기가 빌드 타임에 계산한다.** 런타임 구축 비용을 0으로 만드는 것이 DataAsset을 고른 이유다.
- 기반 클래스는 `--asset-base`로 받으며 기본값은 `UPrimaryDataAsset`이다. **프로젝트 고유 타입을 박지 않는다**(이 생성기는 프로젝트에 종속되지 않는다).
- **기반 클래스를 바꾸면 헤더도 함께 줘야 한다.** 기본값이면 `#include "Engine/DataAsset.h"`를 자동으로 넣지만, `--asset-base`를 다른 클래스로 바꾸면서 `--asset-base-header`를 빠뜨리면 **사용 오류(종료코드 2)로 멈춘다**. 컴파일되지 않는 코드를 조용히 내보내지 않기 위해서다. 헤더 값은 해석·검증 없이 그대로 include 문에 들어간다(`--asset-base-header "TableData/DtTableAsset.h"` → `#include "TableData/DtTableAsset.h"`).
- 클래스 이름은 `U<접두사><테이블>Table`, 파일은 `<접두사><테이블>Table.h`.
- 행 구조체와 마찬가지로 **클라 범위(`C`,`B`) 필드만** 들어간다.
- **모든 필드는 선언과 함께 초기화한다.** 선언된 열 기본값이 있으면 그 값을 C++ 리터럴·생성식으로 내보낸다. 선언이 없으면 기존대로 숫자는 `= 0`, `bool`은 `= false`, 열거형은 첫 항목, 고정 배열은 `{}`를 쓴다. `FName`·`FString`·`FText`·`FGameplayTag`·`FSoftObjectPath`는 기본값 선언이 없을 때 자체 기본 생성자를 사용한다.

```cpp
int32 Id = 0;
float Damage = 0.0f;
float Multiplier = 1.0f; // 2행이 float=1.0
EDtElement Element = EDtElement::None;
int32 Reward[3] = {};
int32 StartValues[2] = {10, 20}; // 각 물리 열이 int32=10, int32=20
```

### 2.1.1 런타임 연동 — 조회·참조 함수와 등록 헤더 (`--runtime-header`)

`--runtime-header <경로>`를 주면 아래 산출물이 추가된다. 주지 않으면 C++ 산출물은 이 기능 이전과 **바이트까지 같다**.

**조회 계약.** 생성 코드는 테이블 저장소를 직접 알지 않고, 지정한 헤더가 제공하는 아래 세 함수만 부른다. 구현은 프로젝트의 몫이다.

```cpp
namespace DrTableRuntime {
  template <typename TRow, typename TKey> const TRow* FindByKey(const TKey& Key);
  template <typename TRow, typename TKey> TArray<const TRow*> FindAllBySubKey(FName SubKeyName, const TKey& Key);
  template <typename TRow> TConstArrayView<TRow> GetAll();
}
```

**행 구조체에 생기는 함수**(UFUNCTION이 아닌 일반 C++ 멤버, 블루프린트 노출 없음). 클라 범위(`B`/`C`) 필드만 대상이다.

| 함수 | 조건 | 동작 |
|---|---|---|
| `static const F…Row* Find(<기본키 타입> Key)` | 모든 테이블 | `FindByKey` |
| `static TArray<const F…Row*> FindBy<서브키>(<서브키 타입> Key)` | 서브키마다 | `FindAllBySubKey(TEXT("<서브키>"), Key)` |
| `static TConstArrayView<F…Row> GetAll()` | 모든 테이블 | `GetAll` |
| `const F<대상>Row* Get<필드>() const` | `Ref<대상>` 필드 | 대상 `Find` |
| `TArray<const F<대상>Row*> Get<필드>() const` | `Ref<대상.서브키>` 필드 | 대상 `FindBy<서브키>` |
| `Get<필드>(int32 Index) const` | 위 둘의 배열 필드 | 범위 밖이면 nullptr/빈 배열 |

- 참조 값이 **없음 값**(`int32`/`int64`는 `0`, `name`은 `NAME_None`)이면 조회하지 않고 바로 nullptr/빈 배열을 돌려준다. 열거형 대상은 빈 셀이 금지라 검사하지 않는다.
- 테이블끼리 서로 참조할 수 있으므로 행 헤더에는 대상 행을 **전방 선언**만 하고, 정의는 테이블마다 생기는 `<P><T>Row.cpp`에서 대상 행 헤더와 런타임 헤더를 include한다. 테이블별 파일로 나눈 것은 한 테이블이 바뀌어도 그 파일만 다시 컴파일되게 하려는 것이다.
- 생성할 함수 이름(`Find`, `GetAll`, `FindBy<서브키>`, `Get<필드>`)이 같은 테이블의 필드명이나 다른 생성 함수와 겹치면, 그 이름을 만든 헤더 셀 주소와 함께 **생성 오류**다. 옵션이 없으면 검사하지 않는다.

**등록 헤더 `<P>TableRegistration.h`.** 모든 테이블의 에셋 헤더를 include하고, `<P>GeneratedTables` 네임스페이스에 등록 템플릿을 둔다. 레지스트리 타입은 템플릿 인자라 생성기는 프로젝트의 저장소 클래스를 모른다.

```cpp
namespace DtGeneratedTables {
  template <typename TRegistry> void RegisterAll(TRegistry& Registry) {
    Registry.template Register<FDtEffectDefsRow, UDtEffectDefsTable>(TEXT("DA_EffectDefs"), &UDtEffectDefsTable::Rows, &UDtEffectDefsTable::PrimaryKeys)
        .WithSchemaHash(EffectDefsSchemaHash)
        .WithSubKey(TEXT("Category"), &UDtEffectDefsTable::Category_Keys, &UDtEffectDefsTable::Category_Offsets, &UDtEffectDefsTable::Category_Indices);
    // … 모든 테이블, 테이블 이름 순
  }
}
```

레지스트리가 제공해야 하는 것: `Register<행, 에셋>(FName 이름, TArray<행> 에셋::*, TArray<기본키> 에셋::*)`, 그 반환값의 `WithSchemaHash(const TCHAR*)`, `WithSubKey(FName, TArray<키> 에셋::*, TArray<int32> 에셋::*, TArray<int32> 에셋::*)`(자기 자신을 돌려줘 이어 부를 수 있어야 함). 모든 테이블에 스키마 해시가 붙으므로 수동 등록에서 해시를 빠뜨리는 일이 없다. 에셋 이름은 `--asset-name`(기본 `DA_{table}`, `{table}`이 반드시 들어가야 함)으로 정한다.

### 2.2 클라이언트 JSON

```json
{
  "table": "Effects",
  "schema_hash": "sha256:...",
  "primary_key": "Id",
  "sub_keys": [{"name": "Element", "field": "Element",
                "keys": ["Fire", "Water"], "offsets": [0, 1, 2], "indices": [0, 1]}],
  "primary_keys": [1001, 1002],
  "rows": [{"Id": 1001, "Name": "Burn", "Element": "Fire"}]
}
```

클라 JSON에는 **구워진 인덱스가 함께 들어간다** — 커밋릿이 그대로 에셋에 옮긴다. `rows`는 기본키 오름차순으로 정렬해 내보내고 `primary_keys`는 그와 같은 순서다. 서브키의 `offsets` 길이는 `keys` 길이 + 1이다.

### 2.3 서버 JSON

같은 형식이되 서버 범위(`S`,`B`) 필드만 담는다. **서버 JSON에는 인덱스를 넣지 않는다**(에셋으로 굽지 않으므로). 클라와 서버 파일은 **별도 디렉터리**에 쓴다.

### 2.4 매니페스트

각 출력 디렉터리에 `manifest.json`:

```json
{
  "source_files": ["Tables.xlsx"],
  "tables": [{"name": "Effects", "rows": 42, "schema_hash": "sha256:..."}],
  "enums": [{"name": "Element", "values": 6}],
  "references": [{"table": "Effects", "field": "Item", "target": "Items", "target_key": null, "cardinality": "one", "key_type": "int32", "array_length": 1, "subkey": false}]
}
```

**타임스탬프는 기본적으로 넣지 않는다.** 생성 시각을 넣으면 같은 입력인데도 결과 바이트가 달라져 결정성 규칙과 충돌하기 때문이다. 이력이 필요하면 호출자가 `--stamp <ISO8601>`로 값을 주입하고, 그때만 `generated_at` 항목이 추가된다(CI가 빌드 시각이나 커밋 해시를 넘기는 용도). 값을 주입한 경우는 결정성 검사 대상에서 제외한다.

`references`에는 해당 출력 범위(C/S)에 실제로 나가는 참조 필드만 들어간다. `target_key`는 기본키 참조면 `null`, 서브키 참조면 필드명이다. `cardinality`는 각각 `one`·`many`다. `array_length`에는 원소 수를 기록하고, 참조 필드 자체가 서브키이면 `subkey: true`로 표시한다. 테이블명·필드명 순으로 정렬한다. 생성 C++의 참조 `UPROPERTY`에는 `meta = (TableRef = "Items")`가 붙는다. 서브키 참조에는 `meta = (TableRef = "DropTable", TableRefKey = "GroupId")`가 붙는다. 고정 배열의 블루프린트 비노출 규칙은 그대로다.

`source_files`는 입력 경로 전체가 아니라 **파일 이름만** 넣는다(작업 디렉터리가 달라도 산출물이 같도록).

`schema_hash`는 **필드명·자료형·키 지정·범위·선언된 열 기본값**으로 계산한다(데이터 값은 제외). 기본값 선언이 없는 기존 스키마는 이전 해시를 그대로 유지한다. 나중에 인앱 패치에서 클라이언트가 자기 코드와 페이로드가 맞는지 확인하는 데 쓴다.

---

## 3. CLI

```
drtable build  --input <xlsx 파일 또는 폴더> --out-cpp <dir> --out-client <dir> --out-server <dir> [--prefix Dt] [--stamp <ISO8601>] [--asset-base UPrimaryDataAsset] [--asset-base-header <경로>] [--runtime-header <경로>] [--asset-name DA_{table}]
drtable graph --input <xlsx 파일 또는 폴더> --out <file.md>
drtable check --client <생성된 클라 JSON 디렉터리> [--server <생성된 서버 JSON 디렉터리>]
drtable check --input <xlsx 파일 또는 폴더>         # 기존 스키마 검사
drtable --version
```

- `graph`는 참조가 없는 테이블도 포함하는 Mermaid `flowchart LR` Markdown을 쓴다. 노드에 기본키 자료형, 화살표에 필드명·배열 길이·참조 필드의 서브키 여부를 표시한다. 대상 서브키를 참조하는 화살표에는 대상 키 이름과 `1:N`을 표시한다.
- `check --client`는 엑셀을 읽지 않고 생성된 JSON과 manifest만 검사한다. 모든 끊긴 참조를 `테이블.필드[행 기본키](배열 인덱스) = 값 → 대상 테이블에 없음` 형태로 출력한다. 없음 값(숫자 `0`·빈 이름)은 건너뛴다. 대상 기본키나 대상 서브키가 없음 값과 충돌하면 경고한다. 서브키 참조는 대상 키 값 집합에 값이 하나라도 있으면 통과하고, 실패 시 `→ 대상테이블.서브키에 해당 값 없음`처럼 대상 테이블·서브키 이름을 함께 출력한다. 서버 경로를 주면 양쪽을 각각 검사한다.
- 종료 코드: `build`·`graph`·기존 스키마 검사에서 0 성공, 1 검증 실패, 2 사용 오류. 참조 검사에서 0 통과, 1 끊긴 참조, 2 입력 오류.
- 오류는 **시트 이름과 셀 주소**(`Effects!C7`)를 반드시 포함한다.
- 출력 디렉터리는 생성 전에 기존 산출물을 지우고 새로 쓴다(삭제된 테이블이 남지 않도록).

## 4. v0.1 검증 범위

생성기는 스키마 오류를 검사한다. 테이블 간 참조 값의 검증은 별도 명령 `drtable check --client`가 맡는다.

- 기본키 없음 / 2개 이상 / 기본키 범위가 `B`가 아님
- 기본키 값 중복, 빈 값
- 필드명 중복, C++ 식별자로 쓸 수 없는 이름
- 알 수 없는 자료형, 없는 열거형 참조
- 열거형 값 중복
- 자료형과 맞지 않는 셀 값
- 자료형으로 변환할 수 없는 열 기본값, 키 열의 기본값 선언
- 서브키 이름 중복
- 배열 열: 번호 빈틈(`[0]`,`[1]`,`[3]`), 번호 중복, 구성 열의 자료형 불일치, 범위 불일치
- 배열 열에 `ID<>`·`SubKey<>` 지정
- 열거형 시트에 기본키 열이 없거나 두 개 이상
- 열거자 이름이 C++ 식별자가 아님, 열거자 이름 중복
- `이름Info` 테이블 이름이 다른 시트와 충돌

**버전·마이그레이션은 지원하지 않는다.** 스키마가 바뀌면 코드와 데이터를 같이 새로 뽑는다. 불일치는 크래시로 드러내고 즉시 대응한다.

## 5. 범위 밖(지금은 안 함)

- 지역화 처리(정책 미정)
- 바이너리 페이로드(JSON으로 시작, 필요해지면 교체)
- `.uasset` 생성 — **생성기는 하지 않는다.** UE 에디터 커밋릿이 이 생성기의 클라 JSON을 읽어 `UPrimaryDataAsset`을 굽는다(2026-09-17 확정). 엔진이 직렬화를 책임져야 UE 버전이 포맷을 바꿔도 깨지지 않기 때문이다. 파이프라인은 `Tables.xlsx → drtable(C++ 헤더 + JSON) → 에디터 커밋릿 → DA_*.uasset → 쿠킹`이다.
- 인앱 패치 적용 로직(설계만 열어 둠). 패치도 같은 클라 JSON을 CDN으로 받아 안전 시점에 테이블 통째로 덮는 방식이다.
- 중첩 구조체 필드(배열은 1.2.1로 지원한다)
