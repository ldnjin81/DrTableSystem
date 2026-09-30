// Copyright ldnjin81. All Rights Reserved.

#if WITH_DEV_AUTOMATION_TESTS

#include "Misc/AutomationTest.h"
#include "TableGenRegistry.h"
#include "TableGenRuntime.h"
#include "Tests/TableGenTestTypes.h"
#include "UObject/Package.h"

namespace TableGenTests
{
	constexpr EAutomationTestFlags Flags = EAutomationTestFlags_ApplicationContextMask | EAutomationTestFlags::ProductFilter;

	void FillAsset(UTableGenTestAsset& Asset)
	{
		FTableGenTestRow First;
		First.Id = 10;
		First.Group = ETableGenTestGroup::ZShared;
		First.Code = TEXT("FirstCode");
		FTableGenTestRow Second;
		Second.Id = 20;
		Second.Group = ETableGenTestGroup::ZShared;
		Second.Code = TEXT("SecondCode");

		Asset.Rows = {First, Second};
		Asset.PrimaryKeys = {10, 20};
		// Sorted by enum value (ZShared=0, AOther=1), not by name.
		Asset.Group_Keys = {ETableGenTestGroup::ZShared, ETableGenTestGroup::AOther};
		Asset.Group_Offsets = {0, 2, 2};
		Asset.Group_Indices = {0, 1};
		Asset.Code_Keys = {TEXT("FirstCode"), TEXT("SecondCode")};
		Asset.Code_Offsets = {0, 1, 2};
		Asset.Code_Indices = {0, 1};
	}

	/** A registry that only loads what a test adds to ExtraAssets. */
	UTableGenRegistry* NewIsolatedRegistry()
	{
		UTableGenRegistry* Registry = NewObject<UTableGenRegistry>();
		Registry->AssetRoot.Reset();
		Registry->ExtraAssets.Reset();
		return Registry;
	}

	TTableGenRowTable<FTableGenTestRow>& RegisterAsset(UTableGenRegistry& Registry, FName TableId)
	{
		return Registry.Register<FTableGenTestRow, UTableGenTestAsset>(TableId, &UTableGenTestAsset::Rows, &UTableGenTestAsset::PrimaryKeys)
			.WithSubKey(TEXT("Group"), &UTableGenTestAsset::Group_Keys, &UTableGenTestAsset::Group_Offsets, &UTableGenTestAsset::Group_Indices)
			.WithUniqueSubKey(TEXT("Code"), &UTableGenTestAsset::Code_Keys, &UTableGenTestAsset::Code_Offsets, &UTableGenTestAsset::Code_Indices);
	}

	struct FFixture
	{
		FFixture()
		{
			static int32 Serial = 0;
			const FName TableId(*FString::Printf(TEXT("TableGenTest_%d"), Serial++));
			Asset = NewObject<UTableGenTestAsset>(GetTransientPackage(), TableId);
			FillAsset(*Asset);
			Registry = NewIsolatedRegistry();
			RegisterAsset(*Registry, TableId);
			Registry->ExtraAssets.Add(TSoftObjectPtr<UPrimaryDataAsset>(Asset));
			Registry->LoadAllTables();
		}

		FName Id() const { return Asset->GetFName(); }

		TObjectPtr<UTableGenTestAsset> Asset;
		TObjectPtr<UTableGenRegistry> Registry;
	};

	/** Restores the global registry even if a test returns early. */
	struct FScopedOverride
	{
		explicit FScopedOverride(UTableGenRegistry* Registry) { UTableGenRegistry::SetOverride(Registry); }
		~FScopedOverride() { UTableGenRegistry::ClearOverride(); }
	};
} // namespace TableGenTests

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FTableGenPrimaryKeyTest, "TableGen.Registry.PrimaryKey", TableGenTests::Flags)
bool FTableGenPrimaryKeyTest::RunTest(const FString&)
{
	TableGenTests::FFixture Fixture;
	const FTableGenTestRow* Row = Fixture.Registry->FindRowByKey<FTableGenTestRow>(20);
	TestNotNull(TEXT("binary search on baked primary keys"), Row);
	if (Row)
	{
		TestEqual(TEXT("row value"), Row->Id, 20);
	}
	TestNotNull(TEXT("lookup by table id"), Fixture.Registry->FindRowByKey<FTableGenTestRow>(Fixture.Id(), 10));
	TestNotNull(TEXT("lookup by key text"), Fixture.Registry->FindRow<FTableGenTestRow>(Fixture.Id(), TEXT("10")));
	TestNull(TEXT("missing key"), Fixture.Registry->FindRowByKey<FTableGenTestRow>(15));
	TestNull(TEXT("key type must match exactly"), Fixture.Registry->FindRowByKey<FTableGenTestRow>(int64{20}));
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FTableGenSubKeyOneToManyTest, "TableGen.Registry.SubKey.OneToMany", TableGenTests::Flags)
bool FTableGenSubKeyOneToManyTest::RunTest(const FString&)
{
	TableGenTests::FFixture Fixture;
	const TArray<const FTableGenTestRow*> Rows = Fixture.Registry->FindRowsBySubKey<FTableGenTestRow>(TEXT("Group"), ETableGenTestGroup::ZShared);
	TestEqual(TEXT("enum sub key sorted by value"), Rows.Num(), 2);
	TestEqual(TEXT("empty bucket"), Fixture.Registry->FindRowsBySubKey<FTableGenTestRow>(TEXT("Group"), ETableGenTestGroup::AOther).Num(), 0);
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FTableGenSubKeyOneToOneTest, "TableGen.Registry.SubKey.OneToOne", TableGenTests::Flags)
bool FTableGenSubKeyOneToOneTest::RunTest(const FString&)
{
	TableGenTests::FFixture Fixture;
	const TArray<const FTableGenTestRow*> Rows = Fixture.Registry->FindRowsBySubKey<FTableGenTestRow>(TEXT("Code"), FName(TEXT("SecondCode")));
	TestEqual(TEXT("one row"), Rows.Num(), 1);
	if (Rows.Num() == 1)
	{
		TestEqual(TEXT("the right row"), Rows[0]->Id, 20);
	}
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FTableGenNameOrderingTest, "TableGen.Registry.NameKey.CodePointOrdering", TableGenTests::Flags)
bool FTableGenNameOrderingTest::RunTest(const FString&)
{
	// The generator's order (Python code point order); FName::LexicalLess would disagree.
	const TArray<FName> Keys = {TEXT("Apple"), TEXT("Cherry"), TEXT("Foo_10"), TEXT("Foo_9"), TEXT("banana")};
	UTableGenTestAsset* Asset = NewObject<UTableGenTestAsset>(GetTransientPackage(), TEXT("TableGenNameOrdering"));
	Asset->Code_Keys = Keys;
	for (int32 Index = 0; Index < Keys.Num(); ++Index)
	{
		FTableGenTestRow& Row = Asset->Rows.AddDefaulted_GetRef();
		Row.Id = Index;
		Row.Code = Keys[Index];
		Asset->Code_Offsets.Add(Index);
		Asset->Code_Indices.Add(Index);
	}
	Asset->Code_Offsets.Add(Keys.Num());

	UTableGenRegistry* Registry = TableGenTests::NewIsolatedRegistry();
	Registry->Register<FTableGenTestRow, UTableGenTestAsset>(Asset->GetFName(), &UTableGenTestAsset::Rows, &UTableGenTestAsset::Code_Keys)
		.WithUniqueSubKey(TEXT("Code"), &UTableGenTestAsset::Code_Keys, &UTableGenTestAsset::Code_Offsets, &UTableGenTestAsset::Code_Indices);
	Registry->ExtraAssets.Add(TSoftObjectPtr<UPrimaryDataAsset>(Asset));
	Registry->LoadAllTables();

	for (int32 Index = 0; Index < Keys.Num(); ++Index)
	{
		const FString Name = Keys[Index].ToString();
		const FTableGenTestRow* Row = Registry->FindRowByKey<FTableGenTestRow>(Keys[Index]);
		TestNotNull(*FString::Printf(TEXT("name primary key %s"), *Name), Row);
		if (Row)
		{
			TestEqual(*FString::Printf(TEXT("name primary key row %s"), *Name), Row->Id, Index);
		}
		TestEqual(*FString::Printf(TEXT("name sub key %s"), *Name), Registry->FindRowsBySubKey<FTableGenTestRow>(TEXT("Code"), Keys[Index]).Num(), 1);
	}
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FTableGenSubKeyNotDeclaredTest, "TableGen.Registry.SubKey.NotDeclared", TableGenTests::Flags)
bool FTableGenSubKeyNotDeclaredTest::RunTest(const FString&)
{
	UTableGenNoSubKeyTestAsset* Asset = NewObject<UTableGenNoSubKeyTestAsset>(GetTransientPackage(), TEXT("TableGenNoSubKey"));
	FTableGenTestRow Row;
	Row.Id = 10;
	Asset->Rows.Add(Row);
	Asset->PrimaryKeys.Add(10);
	UTableGenRegistry* Registry = TableGenTests::NewIsolatedRegistry();
	Registry->Register<FTableGenTestRow, UTableGenNoSubKeyTestAsset>(Asset->GetFName(), &UTableGenNoSubKeyTestAsset::Rows, &UTableGenNoSubKeyTestAsset::PrimaryKeys);
	Registry->ExtraAssets.Add(TSoftObjectPtr<UPrimaryDataAsset>(Asset));
	Registry->LoadAllTables();
	TestEqual(TEXT("undeclared sub key is empty"), Registry->FindRowsBySubKey<FTableGenTestRow>(TEXT("Missing"), 123).Num(), 0);
	TestNotNull(TEXT("primary key still works"), Registry->FindRowByKey<FTableGenTestRow>(10));
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FTableGenReloadTest, "TableGen.Registry.ReloadUsesBakedIndices", TableGenTests::Flags)
bool FTableGenReloadTest::RunTest(const FString&)
{
	TableGenTests::FFixture Fixture;
	Fixture.Asset->Rows.RemoveAt(0);
	Fixture.Asset->PrimaryKeys.RemoveAt(0);
	Fixture.Asset->Group_Offsets = {0, 1, 1};
	Fixture.Asset->Group_Indices = {0};
	Fixture.Asset->Code_Keys = {TEXT("SecondCode")};
	Fixture.Asset->Code_Offsets = {0, 1};
	Fixture.Asset->Code_Indices = {0};
	Fixture.Registry->ReloadAllTables();
	TestNull(TEXT("removed key after reload"), Fixture.Registry->FindRowByKey<FTableGenTestRow>(10));
	TestEqual(TEXT("baked CSR after reload"), Fixture.Registry->FindRowsBySubKey<FTableGenTestRow>(TEXT("Group"), ETableGenTestGroup::ZShared).Num(), 1);
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FTableGenInvalidIndexTest, "TableGen.Registry.InvalidIndexIsDisabled", TableGenTests::Flags)
bool FTableGenInvalidIndexTest::RunTest(const FString&)
{
	TableGenTests::FFixture Fixture;
	Fixture.Asset->Group_Offsets.Reset();
	Fixture.Asset->Group_Indices.Reset();
	AddExpectedMessage(TEXT("Invalid CSR index lengths"), ELogVerbosity::Warning, EAutomationExpectedErrorFlags::Contains, 1);
	Fixture.Registry->ReloadAllTables();
	TestEqual(TEXT("broken index is not rebuilt at runtime"),
		Fixture.Registry->FindRowsBySubKey<FTableGenTestRow>(TEXT("Group"), ETableGenTestGroup::ZShared).Num(), 0);
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FTableGenAmbiguousTest, "TableGen.Registry.AmbiguousRowType", TableGenTests::Flags)
bool FTableGenAmbiguousTest::RunTest(const FString&)
{
	TableGenTests::FFixture Fixture;
	UTableGenTestAsset* Second = NewObject<UTableGenTestAsset>(GetTransientPackage(), TEXT("TableGenSecond"));
	TableGenTests::FillAsset(*Second);
	TableGenTests::RegisterAsset(*Fixture.Registry, Second->GetFName());
	Fixture.Registry->ExtraAssets.Add(TSoftObjectPtr<UPrimaryDataAsset>(Second));
	Fixture.Registry->ReloadAllTables();
	AddExpectedMessage(TEXT("registered for more than one table"), ELogVerbosity::Warning, EAutomationExpectedErrorFlags::Contains, 1);
	TestEqual(TEXT("type-only lookup refuses to guess"), Fixture.Registry->GetRows<FTableGenTestRow>().Num(), 0);
	TestNotNull(TEXT("lookup by table id still works"), Fixture.Registry->FindRowByKey<FTableGenTestRow>(Second->GetFName(), 20));
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FTableGenSchemaHashTest, "TableGen.Registry.SchemaHash", TableGenTests::Flags)
bool FTableGenSchemaHashTest::RunTest(const FString&)
{
	TableGenTests::FFixture Fixture;
	TableGenTests::RegisterAsset(*Fixture.Registry, Fixture.Id()).WithSchemaHash(TEXT("sha256:schema"));
	Fixture.Asset->SchemaHash = TEXT("sha256:schema");
	Fixture.Registry->ReloadAllTables();
	TestNotNull(TEXT("matching schema loads"), Fixture.Registry->FindRowByKey<FTableGenTestRow>(20));

	AddExpectedError(TEXT("Schema mismatch, re-bake required"), EAutomationExpectedErrorFlags::Contains, 2);
	Fixture.Asset->SchemaHash = TEXT("sha256:old");
	Fixture.Registry->ReloadAllTables();
	TestNull(TEXT("different schema is not loaded"), Fixture.Registry->FindRowByKey<FTableGenTestRow>(20));
	Fixture.Asset->SchemaHash.Reset();
	Fixture.Registry->ReloadAllTables();
	TestNull(TEXT("empty schema is not loaded"), Fixture.Registry->FindRowByKey<FTableGenTestRow>(20));
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FTableGenContentHashTest, "TableGen.Registry.ContentHash", TableGenTests::Flags)
bool FTableGenContentHashTest::RunTest(const FString&)
{
	TableGenTests::FFixture Fixture;
	TableGenTests::RegisterAsset(*Fixture.Registry, Fixture.Id()).WithContentHash(TEXT("sha256:new-data"));
	Fixture.Asset->ContentHash = TEXT("sha256:new-data");
	Fixture.Registry->ReloadAllTables();
	TestNotNull(TEXT("matching content loads"), Fixture.Registry->FindRowByKey<FTableGenTestRow>(20));

	AddExpectedMessage(TEXT("Data changed since the asset was baked"), ELogVerbosity::Warning, EAutomationExpectedErrorFlags::Contains, 1);
	Fixture.Asset->ContentHash = TEXT("sha256:old-data");
	Fixture.Registry->ReloadAllTables();
	TestNotNull(TEXT("stale content still loads (warning only)"), Fixture.Registry->FindRowByKey<FTableGenTestRow>(20));
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FTableGenRuntimeContractTest, "TableGen.Runtime.Contract", TableGenTests::Flags)
bool FTableGenRuntimeContractTest::RunTest(const FString&)
{
	TableGenTests::FFixture Fixture;
	{
		TableGenTests::FScopedOverride Scope(Fixture.Registry);
		const FTableGenTestRow* Row = TableGenRuntime::FindByKey<FTableGenTestRow>(20);
		TestNotNull(TEXT("FindByKey"), Row);
		TestEqual(TEXT("FindAllBySubKey"), TableGenRuntime::FindAllBySubKey<FTableGenTestRow>(TEXT("Group"), ETableGenTestGroup::ZShared).Num(), 2);
		TestEqual(TEXT("GetAll"), TableGenRuntime::GetAll<FTableGenTestRow>().Num(), 2);
	}
	{
		TableGenTests::FScopedOverride Scope(nullptr);
		TestNull(TEXT("no registry: FindByKey"), TableGenRuntime::FindByKey<FTableGenTestRow>(20));
		TestEqual(TEXT("no registry: FindAllBySubKey"), TableGenRuntime::FindAllBySubKey<FTableGenTestRow>(TEXT("Group"), ETableGenTestGroup::ZShared).Num(), 0);
		TestEqual(TEXT("no registry: GetAll"), TableGenRuntime::GetAll<FTableGenTestRow>().Num(), 0);
	}
	return true;
}

#endif // WITH_DEV_AUTOMATION_TESTS
