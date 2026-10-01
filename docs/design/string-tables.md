# 스트링테이블 설계 (초안)

> **변경(2026-10-01, 사용자 결정)**: 스키마는 `<이름>.string.xlsx` **언어 목록**(A 언어 코드, B 기준 표시, C 범위; 자료형 없음)으로 나눴다. 이 파일이 정의하는 테이블은 `<이름>String`이고(`UI.string.xlsx` → `UIString`), 스트링 폴더의 시트 `UI`가 그 데이터다. 아래 본문의 `lang`/`Base<lang>` 열 표기는 내부 표현으로만 남았고, 일반 `.schema.xlsx`에는 쓸 수 없다. 언어 코드는 `zh-Hans`처럼 써도 된다.

상태: **구현됨**(2026-10-01). 결정(◆)은 모두 확정했다. 사용법은 매뉴얼 8절이 기준이고, 이 문서는 설계 기록이다.

확정 사항:
- 기준 언어는 스키마의 `Base<lang>`으로 정한다.
- 빈 번역은 빌드할 때 기준 언어로 채운다.
- 키 상수 헤더는 **옵션(`--string-keys`)**이다. 원래 추천은 "항상"이었지만, 키가 데이터에서 오므로 항상 만들면 "데이터 수정은 코드를 바꾸지 않는다"는 원칙에 어긋난다.
- 시작 언어: 저장된 선택 → 엔진 컬처 → 기본 언어. 엔진 컬처 추종은 설정이며 기본은 끔이다.
- 서브시스템은 GameInstance 범위다.
- 실행 시 조회는 정렬 이진 탐색 대신 로딩 때 만든 `TMap<FName,int32>`를 쓴다.
- 테이블에 없는 언어는 그 테이블의 기준 언어 에셋으로 대신한다(굽기가 언어 목록 에셋에 기록).

## 1. 요구 사항

- 키는 `name`(FName). 대부분 클라이언트에서 쓴다.
- 언어마다 필드(열)가 있다.
- **언어별로 따로 로딩**한다. 런타임에 설정 언어를 바꾸면 그 언어로 교체한다.
- 설정되지 않은 언어는 **메모리에 올리지 않는다**(언로드 상태).
- 새 언어 로딩이 끝나면 UI가 다시 그리도록 **델리게이트**로 알린다.

## 2. 스키마와 데이터 (엑셀)

새 자료형 `lang` 하나만 추가한다. 스키마 형식, 폴더, 분할 규칙은 기존 테이블과 같다.

`Schema/UIStrings.schema.xlsx`

| Field | Type | Scope | Comment |
|---|---|---|---|
| Id | `ID<name>` | client | |
| ko | `Base<lang>` | client | 기준 언어 |
| en | `lang` | client | |
| ja | `lang` | client | |
| zh_Hans | `lang` | client | 컬처 `zh-Hans` |

- `lang` 열이 하나라도 있으면 그 테이블은 **스트링테이블**이다.
- **기준 언어는 스키마에서 정한다**: 언어 열 하나를 `Base<lang>`으로 적는다. `ID<…>`, `SubKey<…>`처럼 역할이 자료형을 감싸는 기존 문법과 같은 모양이다.
  - 스트링테이블마다 정확히 하나여야 한다(없거나 둘 이상이면 스키마 오류).
  - 테이블마다 기준 언어가 달라도 된다(예: 시스템 문구는 `en` 기준). 빈 번역은 그 테이블의 기준 언어로 채운다.
  - 빌드 옵션이 필요 없고, 기준을 바꾸는 것도 스키마 변경이라 프로그래머 검수를 거친다.
  - 허용하는 열은 기본키(`ID<name>`)와 `lang` 열뿐이다. `#` 메모 열은 지금처럼 자유롭게 쓴다.
  - 서브키, 참조, 일반 필드는 오류로 막는다. 문자열과 데이터가 섞이면 언어별 로딩이 깨지기 때문이다.
- 필드 이름이 곧 컬처 코드다. 식별자에 `-`를 쓸 수 없으므로 `_`를 `-`로 바꿔 읽는다(`zh_Hans` → `zh-Hans`, `pt_BR` → `pt-BR`).
- 데이터 엑셀 형식은 기존과 같다. 1행 필드명, 4행부터 데이터를 적고, 열 순서는 자유다. `UIStrings#메뉴`처럼 시트와 파일을 나눌 수 있다.

### 폴더: 스트링테이블 데이터는 전용 폴더에 둔다
```
Table/            일반 데이터 엑셀(하위 폴더 자유)
Table/Schema/     테이블 스키마 — 스트링테이블 스키마(Base<lang>)도 여기
Table/Enums/      열거형 스키마
Table/Strings/    스트링테이블 데이터 엑셀(하위 폴더 자유)
```
- 기본 위치는 열거형 폴더처럼 **스키마 폴더 옆의 `Strings`**이다. `--schema`를 주지 않으면 `<input>/Strings`를 쓴다. 다른 곳을 쓰려면 `--strings <폴더>`로 지정한다.
- 일반 데이터를 훑을 때 이 폴더는 건너뛰고, 따로 읽는다.
- 섞임은 막는다.
  - 스트링테이블 시트가 `Strings` 밖에 있으면 오류다.
  - 일반 테이블 시트가 `Strings` 안에 있으면 오류다.
  - 메시지에는 `[파일]시트!A1` 위치와 옮길 폴더를 적는다.
- 스키마는 다른 스키마와 같은 폴더에 둔다. 구조는 프로그래머가 맡는다는 원칙을 그대로 따른다.
- 얻는 것:
  - 번역·현지화 담당이 이 폴더만 받아 작업하고 돌려주면 된다. 외주로 넘기거나 잠금 범위를 정하기도 쉽다.
  - 게임 데이터 수정과 번역 수정이 서로 다른 파일이라 충돌하지 않는다.
  - CI에서 "번역만 바뀜"을 폴더 단위로 알아볼 수 있다.
- 범위(scope): 기본은 client다. 서버가 쓰는 문자열(우편 제목 등)은 `all`로 두면 서버 JSON에도 나간다.

### 빌드 검사 (오류 또는 경고)
| 검사 | 수준 |
|---|---|
| 기준 언어(`Base<lang>` 열) 칸이 비어 있음 | 오류 |
| 기준 언어가 아닌 칸이 비어 있음 | 경고 + 대체값(아래) |
| 언어끼리 서식 인자(`{0}`, `{Name}`)가 다름 | 경고 |
| 키가 대소문자만 다름 | 오류 (기존 name 키 규칙) |

## 3. 산출물

### JSON — 언어별로 파일을 나눈다
```
client/Strings/ko/UIStrings.json     {"table": "UIStrings", "language": "ko", "keys": [...], "values": [...], ...}
client/Strings/en/UIStrings.json
client/manifest.json         "string_tables": [{"name": "UIStrings", "languages": ["ko","en","ja","zh-Hans"], "base": "ko"}]
```
- `keys`는 정렬된 FName 목록이다. 기존 기본키 정렬 규칙(코드 포인트 순)을 그대로 쓴다. `values`는 같은 순서의 문자열 배열이다.
- **비어 있는 번역은 빌드할 때 기준 언어로 채운다**(◆). 그래서 런타임에는 설정 언어 하나만 올라가 있어도 빈칸이 생기지 않는다. 대체 언어를 따로 로딩할 필요가 없다.
- 해시는 언어 파일마다 따로 둔다. schema_hash는 키 집합과 언어 목록으로, content_hash는 그 언어의 값으로 만든다. 한 언어만 고치면 그 언어 에셋만 다시 굽는다.

### C++ — 키를 상수로
```cpp
// DtUIStrings.h (생성)
namespace DtUIStrings
{
    inline const FName Btn_OK = TEXT("Btn_OK");     // 키를 상수로: 오타가 컴파일 오류가 된다
    inline const FName Title_Main = TEXT("Title_Main");
    FText Get(FName Key);                           // UDrStringSubsystem 경유
}
```
- 문자열 **값**은 코드에 넣지 않는다. 키 목록이 바뀔 때만 코드가 바뀐다. "데이터 수정은 코드를 바꾸지 않는다"는 원칙대로다.
- ◆ 키 상수 생성은 선택 사항(`--string-keys`)으로 둘 수도 있다. 키가 수만 개면 헤더가 커지기 때문이다.

## 4. 에셋 (굽기)

- 언어마다, 테이블마다 에셋 하나를 만든다. 경로는 `<AssetRoot>/Strings/<lang>/DA_UIStrings_<lang>`이다.
  - 클래스는 `UDrStringTableAsset : UPrimaryDataAsset`이다. 내용은 `Language`, `TArray<FName> Keys`, `TArray<FString> Values`, `SchemaHash`, `ContentHash`.
- 공통 기본 클래스를 쓰므로 테이블마다 생성되는 클래스가 없다.
- PrimaryAssetType은 `DrStrings`, 번들 이름은 언어 코드다. 언어별로 청크/팩을 나눌 수 있어 언어 DLC를 따로 배포하기 좋다.
- `DrTableBake -Verify`가 언어 에셋도 함께 검사한다.

## 5. 런타임

### `UDrStringSubsystem : UGameInstanceSubsystem`
```cpp
DECLARE_DYNAMIC_MULTICAST_DELEGATE_OneParam(FDrOnLanguageChanged, const FString&, Language);
DECLARE_MULTICAST_DELEGATE_OneParam(FDrOnLanguageChangedNative, const FString&);

UFUNCTION(BlueprintCallable) void SetLanguage(const FString& Language);   // 비동기
UFUNCTION(BlueprintPure)     FString GetLanguage() const;                  // 지금 화면에 쓰는 언어
UFUNCTION(BlueprintPure)     FString GetPendingLanguage() const;           // 로딩 중인 언어(없으면 빈 값)
UFUNCTION(BlueprintPure)     FText GetText(FName Table, FName Key) const;
UFUNCTION(BlueprintPure)     bool TryGetText(FName Table, FName Key, FText& Out) const;
TArray<FString> GetAvailableLanguages() const;                              // manifest 기준

UPROPERTY(BlueprintAssignable) FDrOnLanguageChanged OnLanguageChanged;     // 블루프린트·UMG
FDrOnLanguageChangedNative OnLanguageChangedNative;                         // C++
```

### 언어 교체 순서
1. `SetLanguage("en")` → 모든 스트링테이블의 `en` 에셋을 `FStreamableManager::RequestAsyncLoad`로 한 번에 요청한다.
2. 로딩하는 동안 화면은 **기존 언어 그대로** 둔다. 빈 글자나 키 문자열이 보이는 깜빡임이 없다.
3. 전부 로딩되면 같은 프레임에서 교체한다.
   - 현재 언어 테이블 포인터를 바꾼다.
   - 이전 언어의 스트리머블 핸들을 해제한다. 이후 GC가 메모리에서 내린다.
   - `OnLanguageChanged`를 브로드캐스트한다.
4. 로딩 중에 다른 언어를 요청하면 진행 중인 요청을 취소하고 마지막 요청만 반영한다.
5. 에셋이 없는 언어를 요청하면 경고 로그를 남기고 현재 언어를 유지한다. 델리게이트는 부르지 않는다.

- 시작 언어 결정 순서(◆): 저장된 사용자 설정 → 엔진 컬처(`FInternationalization::GetCurrentCulture`) → 기준 언어.
- 엔진 컬처를 따를지는 설정(`bFollowEngineCulture`)으로 정한다. 켜면 `OnCultureChanged`가 왔을 때 `SetLanguage`를 부른다.
- 첫 언어만 동기 로딩을 허용한다(`LoadInitialLanguageSync`). 타이틀 화면 첫 프레임부터 글자가 나와야 하기 때문이다.

### 조회
- 키를 찾을 때는 정렬된 `Keys`를 이진 탐색한다. 기존 `KeyLess` 계약을 재사용한다. 로딩 시점에 `TMap`을 따로 만들지 않는다.
- 반환형은 `FText::AsCultureInvariant(Value)`다. UE의 자체 로컬라이제이션(.locres)과 섞이지 않게 문화권 불변 텍스트로 넘긴다. `FText::Format`에 그대로 쓸 수 있다.
- 키가 없으면 `"<UIStrings.Missing_Key>"`를 반환하고 키마다 한 번만 경고한다. 개발 빌드에서만 그렇게 하고, 배포 빌드에서는 빈 텍스트를 반환한다(◆).

### 갱신 도우미
- `UDrLocalizedTextBlock : UTextBlock`: `Table`·`Key` 속성을 두고 `OnLanguageChanged`에 스스로 묶여서 다시 그린다. 대부분의 UI는 이것만 쓰면 된다.
- 다른 테이블에서 문자열을 가리킬 때는 기존 참조를 그대로 쓴다. `ItemName: Ref<ItemStrings>` 같은 식이다. 생성되는 `GetItemNameText()`는 서브시스템을 거쳐 현재 언어의 텍스트를 돌려준다(2단계).

## 6. 메모리 수명 요약
| 상태 | 메모리 |
|---|---|
| 현재 언어 | 모든 스트링테이블의 해당 언어 에셋이 로딩됨 |
| 교체 중 | 이전 언어와 새 언어가 잠시 함께 있음(교체 직후 이전 언어 해제) |
| 그 밖의 언어 | 로딩 안 됨. 쿠킹은 되어 있어 요청하면 비동기로 올라옴 |

## 7. 단계
1. **생성기**: `lang` 자료형, 스트링테이블 규칙과 검사, 언어별 JSON, manifest, 키 상수 헤더. 테스트는 `rust/tests/strings.rs`.
2. **플러그인**: `UDrStringTableAsset`, 굽기와 -Verify, `UDrStringSubsystem`(비동기 교체·해제·델리게이트), 자동화 테스트(교체, 언로드 확인, 연속 요청 취소).
3. **편의 기능**: `UDrLocalizedTextBlock`, `Ref<스트링테이블>`의 `Get…Text()`, GUI 테이블 탭에서 언어 열 비교 보기.
4. **매뉴얼**: 한·영 "스트링테이블" 장.

## 8. 확인할 결정 (◆)
1. ~~기준 언어~~ → 스키마의 `Base<lang>` 열로 정함(확정)
2. 빈 번역: 빌드할 때 기준 언어로 채우기(추천) / 빈 문자열 / 키 표시
3. 키 상수 헤더를 항상 만들지, 옵션으로 할지
4. 시작 언어 결정 순서와 엔진 컬처 연동 여부
5. 서브시스템 범위: GameInstance(추천, PIE마다 독립) / Engine
