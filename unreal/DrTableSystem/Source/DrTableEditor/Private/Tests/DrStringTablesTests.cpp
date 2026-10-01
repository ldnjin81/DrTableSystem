// Copyright ldnjin81. All Rights Reserved.

#if WITH_DEV_AUTOMATION_TESTS

#include "DrStringSubsystem.h"
#include "DrStringTableAsset.h"
#include "DrStringTables.h"
#include "DrTableRuntime.h"
#include "Misc/AutomationTest.h"
#include "UObject/Package.h"
#include "UObject/UObjectGlobals.h"

namespace DrStringTests
{
	constexpr EAutomationTestFlags Flags = EAutomationTestFlags_ApplicationContextMask | EAutomationTestFlags::ProductFilter;

	UDrStringTableAsset* NewLanguageAsset(FName Table, const FString& Language, TArray<FName> Keys, TArray<FString> Values)
	{
		const FName Name = MakeUniqueObjectName(GetTransientPackage(), UDrStringTableAsset::StaticClass(),
			FName(*FString::Printf(TEXT("DrStringTest_%s_%s"), *Table.ToString(), *Language.Replace(TEXT("-"), TEXT("_")))));
		UDrStringTableAsset* Asset = NewObject<UDrStringTableAsset>(GetTransientPackage(), Name);
		Asset->Table = Table;
		Asset->Language = Language;
		Asset->Keys = MoveTemp(Keys);
		Asset->Values = MoveTemp(Values);
		return Asset;
	}

	/** UI strings in ko (base) and en; the assets are referenced only through the manifest's soft pointers. */
	struct FFixture
	{
		FFixture()
		{
			UDrStringTableAsset* Korean = NewLanguageAsset(TEXT("UI"), TEXT("ko"), {TEXT("Btn_OK"), TEXT("Title")}, {TEXT("확인"), TEXT("제목")});
			UDrStringTableAsset* English = NewLanguageAsset(TEXT("UI"), TEXT("en"), {TEXT("Btn_OK"), TEXT("Title")}, {TEXT("OK"), TEXT("Title")});
			KoreanAsset = Korean;
			EnglishAsset = English;
			Manifest = NewObject<UDrStringManifest>(GetTransientPackage());
			Manifest->DefaultLanguage = TEXT("ko");
			for (UDrStringTableAsset* Asset : {Korean, English})
			{
				FDrStringLanguage& Entry = Manifest->Languages.AddDefaulted_GetRef();
				Entry.Language = Asset->Language;
				Entry.Tables.Add(TSoftObjectPtr<UDrStringTableAsset>(Asset));
			}
			Manifest->AddToRoot();
			Tables = NewObject<UDrStringTables>(GetTransientPackage());
			Tables->AddToRoot();
			Tables->SetManifest(Manifest);
			Tables->OnLanguageChanged.AddLambda([this](const FString& Language) { Changes.Add(Language); });
		}

		~FFixture()
		{
			Tables->RemoveFromRoot();
			Manifest->RemoveFromRoot();
		}

		UDrStringTables* Tables = nullptr;
		UDrStringManifest* Manifest = nullptr;
		TWeakObjectPtr<UDrStringTableAsset> KoreanAsset;
		TWeakObjectPtr<UDrStringTableAsset> EnglishAsset;
		TArray<FString> Changes;
	};

	struct FScopedOverride
	{
		explicit FScopedOverride(UDrStringTables* Tables) { UDrStringTables::SetOverride(Tables); }
		~FScopedOverride() { UDrStringTables::ClearOverride(); }
	};
} // namespace DrStringTests

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrStringLookupTest, "DrTable.Strings.Lookup", DrStringTests::Flags)
bool FDrStringLookupTest::RunTest(const FString&)
{
	DrStringTests::FFixture Fixture;
	TestTrue(TEXT("loads the base language"), Fixture.Tables->LoadLanguageSync(TEXT("ko")));
	TestEqual(TEXT("current language"), Fixture.Tables->GetLanguage(), FString(TEXT("ko")));
	TestEqual(TEXT("text"), Fixture.Tables->GetText(TEXT("UI"), TEXT("Btn_OK")).ToString(), FString(TEXT("확인")));
	TestEqual(TEXT("keys are case-insensitive FNames"), Fixture.Tables->GetText(TEXT("UI"), TEXT("btn_ok")).ToString(), FString(TEXT("확인")));
	FText Text;
	TestFalse(TEXT("missing key"), Fixture.Tables->TryGetText(TEXT("UI"), TEXT("Nope"), Text));
	AddExpectedMessage(TEXT("No text for UI.Nope"), ELogVerbosity::Warning, EAutomationExpectedMessageFlags::Contains, 1, false);
	TestEqual(TEXT("missing key text in development builds"), Fixture.Tables->GetText(TEXT("UI"), TEXT("Nope")).ToString(), FString(TEXT("<UI.Nope>")));
	TestTrue(TEXT("empty key gives empty text"), Fixture.Tables->GetText(TEXT("UI"), NAME_None).IsEmpty());
	TestEqual(TEXT("one change"), Fixture.Changes.Num(), 1);
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrStringSwitchTest, "DrTable.Strings.SwitchUnloadsPreviousLanguage", DrStringTests::Flags)
bool FDrStringSwitchTest::RunTest(const FString&)
{
	DrStringTests::FFixture Fixture;
	Fixture.Tables->LoadLanguageSync(TEXT("ko"));
	Fixture.Changes.Reset();
	TestTrue(TEXT("switch"), Fixture.Tables->SetLanguage(TEXT("en")));
	TestEqual(TEXT("current language"), Fixture.Tables->GetLanguage(), FString(TEXT("en")));
	TestTrue(TEXT("nothing pending"), Fixture.Tables->GetPendingLanguage().IsEmpty());
	TestTrue(TEXT("one broadcast"), Fixture.Changes == TArray<FString>{TEXT("en")});
	TestEqual(TEXT("text in the new language"), Fixture.Tables->GetText(TEXT("UI"), TEXT("Btn_OK")).ToString(), FString(TEXT("OK")));
	TestEqual(TEXT("only the current language is held"), Fixture.Tables->GetLoadedAssets().Num(), 1);

	CollectGarbage(GARBAGE_COLLECTION_KEEPFLAGS);
	TestFalse(TEXT("the previous language was unloaded"), Fixture.KoreanAsset.IsValid());
	TestTrue(TEXT("the current language stays"), Fixture.EnglishAsset.IsValid());

	TestTrue(TEXT("same language again"), Fixture.Tables->SetLanguage(TEXT("en")));
	TestEqual(TEXT("no broadcast for the same language"), Fixture.Changes.Num(), 1);
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrStringUnknownLanguageTest, "DrTable.Strings.UnknownLanguage", DrStringTests::Flags)
bool FDrStringUnknownLanguageTest::RunTest(const FString&)
{
	DrStringTests::FFixture Fixture;
	Fixture.Tables->LoadLanguageSync(TEXT("ko"));
	Fixture.Changes.Reset();
	AddExpectedMessage(TEXT("Unknown language 'fr'"), ELogVerbosity::Warning, EAutomationExpectedMessageFlags::Contains, 1, false);
	TestFalse(TEXT("unknown language"), Fixture.Tables->SetLanguage(TEXT("fr")));
	TestEqual(TEXT("keeps the current language"), Fixture.Tables->GetLanguage(), FString(TEXT("ko")));
	TestEqual(TEXT("no broadcast"), Fixture.Changes.Num(), 0);
	TestTrue(TEXT("available languages"), Fixture.Tables->GetAvailableLanguages() == TArray<FString>{TEXT("ko"), TEXT("en")});
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrStringRuntimeTextTest, "DrTable.Strings.RuntimeGetText", DrStringTests::Flags)
bool FDrStringRuntimeTextTest::RunTest(const FString&)
{
	DrStringTests::FFixture Fixture;
	Fixture.Tables->LoadLanguageSync(TEXT("ko"));
	{
		DrStringTests::FScopedOverride Override(Fixture.Tables);
		TestEqual(TEXT("generated accessors' path"), DrTableRuntime::GetText(TEXT("UI"), TEXT("Title")).ToString(), FString(TEXT("제목")));
	}
	{
		DrStringTests::FScopedOverride Override(nullptr);
		TestTrue(TEXT("no string tables: empty text"), DrTableRuntime::GetText(TEXT("UI"), TEXT("Title")).IsEmpty());
	}
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrStringChooseLanguageTest, "DrTable.Strings.ChooseLanguage", DrStringTests::Flags)
bool FDrStringChooseLanguageTest::RunTest(const FString&)
{
	const TArray<FString> Available = {TEXT("ko"), TEXT("en"), TEXT("zh-Hans")};
	TestEqual(TEXT("saved choice first"), UDrStringSubsystem::ChooseLanguage(Available, TEXT("en"), TEXT("ko-KR"), TEXT("ko")), FString(TEXT("en")));
	TestEqual(TEXT("culture parent"), UDrStringSubsystem::ChooseLanguage(Available, TEXT(""), TEXT("ko-KR"), TEXT("en")), FString(TEXT("ko")));
	TestEqual(TEXT("exact culture"), UDrStringSubsystem::ChooseLanguage(Available, TEXT(""), TEXT("zh-Hans-CN"), TEXT("ko")), FString(TEXT("zh-Hans")));
	TestEqual(TEXT("saved choice no longer available"), UDrStringSubsystem::ChooseLanguage(Available, TEXT("fr"), TEXT("de-DE"), TEXT("ko")), FString(TEXT("ko")));
	TestTrue(TEXT("no languages"), UDrStringSubsystem::ChooseLanguage({}, TEXT(""), TEXT("ko"), TEXT("ko")).IsEmpty());
	return true;
}

#endif // WITH_DEV_AUTOMATION_TESTS
