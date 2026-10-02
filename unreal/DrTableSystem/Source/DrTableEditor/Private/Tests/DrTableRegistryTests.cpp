// Copyright ldnjin81. All Rights Reserved.

#if WITH_DEV_AUTOMATION_TESTS

#include "Misc/AutomationTest.h"
#include "DrTableRegistry.h"
#include "DrTableRuntime.h"
#include "Tests/DrTableTestTypes.h"
#include "UObject/Package.h"

namespace DrTableTests
{
	constexpr EAutomationTestFlags Flags = EAutomationTestFlags_ApplicationContextMask | EAutomationTestFlags::ProductFilter;

	void FillAsset(UDrTableTestAsset& Asset)
	{
		FDrTableTestRow First;
		First.Id = 10;
		First.Group = EDrTableTestGroup::ZShared;
		First.Code = TEXT("FirstCode");
		FDrTableTestRow Second;
		Second.Id = 20;
		Second.Group = EDrTableTestGroup::ZShared;
		Second.Code = TEXT("SecondCode");

		Asset.Rows = {First, Second};
		Asset.PrimaryKeys = {10, 20};
		// Sorted by enum value (ZShared=0, AOther=1), not by name.
		Asset.Group_Keys = {EDrTableTestGroup::ZShared, EDrTableTestGroup::AOther};
		Asset.Group_Offsets = {0, 2, 2};
		Asset.Group_Indices = {0, 1};
		Asset.Code_Keys = {TEXT("FirstCode"), TEXT("SecondCode")};
		Asset.Code_Offsets = {0, 1, 2};
		Asset.Code_Indices = {0, 1};
		// Array field Reward: row 10 has [7, 8, 9], row 20 has none.
		Asset.Rows[0].Reward_Start = 0;
		Asset.Rows[0].Reward_Num = 3;
		Asset.Rows[1].Reward_Start = 3;
		Asset.Rows[1].Reward_Num = 0;
		Asset.Reward_Pool = {7, 8, 9};
	}

	/** A registry that only loads what a test adds to ExtraAssets. */
	UDrTableRegistry* NewIsolatedRegistry()
	{
		UDrTableRegistry* Registry = NewObject<UDrTableRegistry>();
		Registry->AssetRoot.Reset();
		Registry->ExtraAssets.Reset();
		return Registry;
	}

	TDrTableRowTable<FDrTableTestRow>& RegisterAsset(UDrTableRegistry& Registry, FName TableId)
	{
		return Registry.Register<FDrTableTestRow, UDrTableTestAsset>(TableId, &UDrTableTestAsset::Rows, &UDrTableTestAsset::PrimaryKeys)
			.WithSubKey(TEXT("Group"), &UDrTableTestAsset::Group_Keys, &UDrTableTestAsset::Group_Offsets, &UDrTableTestAsset::Group_Indices)
			.WithUniqueSubKey(TEXT("Code"), &UDrTableTestAsset::Code_Keys, &UDrTableTestAsset::Code_Offsets, &UDrTableTestAsset::Code_Indices)
			.WithArray(TEXT("Reward"), &UDrTableTestAsset::Reward_Pool);
	}

	struct FFixture
	{
		FFixture()
		{
			static int32 Serial = 0;
			const FName TableId(*FString::Printf(TEXT("DrTableTest_%d"), Serial++));
			Asset = NewObject<UDrTableTestAsset>(GetTransientPackage(), TableId);
			FillAsset(*Asset);
			Registry = NewIsolatedRegistry();
			RegisterAsset(*Registry, TableId);
			Registry->ExtraAssets.Add(TSoftObjectPtr<UPrimaryDataAsset>(Asset));
			Registry->LoadAllTables();
		}

		FName Id() const { return Asset->GetFName(); }

		TObjectPtr<UDrTableTestAsset> Asset;
		TObjectPtr<UDrTableRegistry> Registry;
	};

	/** Restores the global registry even if a test returns early. */
	struct FScopedOverride
	{
		explicit FScopedOverride(UDrTableRegistry* Registry) { UDrTableRegistry::SetOverride(Registry); }
		~FScopedOverride() { UDrTableRegistry::ClearOverride(); }
	};
} // namespace DrTableTests

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrTablePrimaryKeyTest, "DrTable.Registry.PrimaryKey", DrTableTests::Flags)
bool FDrTablePrimaryKeyTest::RunTest(const FString&)
{
	DrTableTests::FFixture Fixture;
	const FDrTableTestRow* Row = Fixture.Registry->FindRowByKey<FDrTableTestRow>(20);
	TestNotNull(TEXT("binary search on baked primary keys"), Row);
	if (Row)
	{
		TestEqual(TEXT("row value"), Row->Id, 20);
	}
	TestNotNull(TEXT("lookup by table id"), Fixture.Registry->FindRowByKey<FDrTableTestRow>(Fixture.Id(), 10));
	TestNotNull(TEXT("lookup by key text"), Fixture.Registry->FindRow<FDrTableTestRow>(Fixture.Id(), TEXT("10")));
	TestNull(TEXT("missing key"), Fixture.Registry->FindRowByKey<FDrTableTestRow>(15));
	TestNull(TEXT("key type must match exactly"), Fixture.Registry->FindRowByKey<FDrTableTestRow>(int64{20}));
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrTableArrayPoolTest, "DrTable.Registry.ArrayPool", DrTableTests::Flags)
bool FDrTableArrayPoolTest::RunTest(const FString&)
{
	DrTableTests::FFixture Fixture;
	const FDrTableTestRow* Row = Fixture.Registry->FindRowByKey<FDrTableTestRow>(10);
	if (!TestNotNull(TEXT("row"), Row))
	{
		return false;
	}
	const TConstArrayView<int32> Items = Fixture.Registry->GetArray<FDrTableTestRow, int32>(TEXT("Reward"), Row->Reward_Start, Row->Reward_Num);
	TestEqual(TEXT("the row's run"), Items.Num(), 3);
	if (Items.Num() == 3)
	{
		TestEqual(TEXT("elements in order"), Items[0] * 100 + Items[1] * 10 + Items[2], 789);
	}
	TestEqual(TEXT("an empty run"), Fixture.Registry->GetArray<FDrTableTestRow, int32>(TEXT("Reward"), 3, 0).Num(), 0);
	TestEqual(TEXT("a run past the pool is empty"), Fixture.Registry->GetArray<FDrTableTestRow, int32>(TEXT("Reward"), 2, 5).Num(), 0);
	TestEqual(TEXT("a negative start is empty"), Fixture.Registry->GetArray<FDrTableTestRow, int32>(TEXT("Reward"), -1, 1).Num(), 0);
	TestEqual(TEXT("an unknown field is empty"), Fixture.Registry->GetArray<FDrTableTestRow, int32>(TEXT("Missing"), 0, 1).Num(), 0);
	TestEqual(TEXT("a different element type is empty"), Fixture.Registry->GetArray<FDrTableTestRow, int64>(TEXT("Reward"), 0, 1).Num(), 0);
	{
		DrTableTests::FScopedOverride Override(Fixture.Registry);
		TestEqual(TEXT("through the runtime contract"), DrTableRuntime::GetArray<FDrTableTestRow, int32>(TEXT("Reward"), 1, 2).Num(), 2);
	}
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrTableSubKeyOneToManyTest, "DrTable.Registry.SubKey.OneToMany", DrTableTests::Flags)
bool FDrTableSubKeyOneToManyTest::RunTest(const FString&)
{
	DrTableTests::FFixture Fixture;
	const TArray<const FDrTableTestRow*> Rows = Fixture.Registry->FindRowsBySubKey<FDrTableTestRow>(TEXT("Group"), EDrTableTestGroup::ZShared);
	TestEqual(TEXT("enum sub key sorted by value"), Rows.Num(), 2);
	TestEqual(TEXT("empty bucket"), Fixture.Registry->FindRowsBySubKey<FDrTableTestRow>(TEXT("Group"), EDrTableTestGroup::AOther).Num(), 0);
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrTableSubKeyOneToOneTest, "DrTable.Registry.SubKey.OneToOne", DrTableTests::Flags)
bool FDrTableSubKeyOneToOneTest::RunTest(const FString&)
{
	DrTableTests::FFixture Fixture;
	const TArray<const FDrTableTestRow*> Rows = Fixture.Registry->FindRowsBySubKey<FDrTableTestRow>(TEXT("Code"), FName(TEXT("SecondCode")));
	TestEqual(TEXT("one row"), Rows.Num(), 1);
	if (Rows.Num() == 1)
	{
		TestEqual(TEXT("the right row"), Rows[0]->Id, 20);
	}
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrTableNameOrderingTest, "DrTable.Registry.NameKey.CodePointOrdering", DrTableTests::Flags)
bool FDrTableNameOrderingTest::RunTest(const FString&)
{
	// The generator's order (Unicode code point order); FName::LexicalLess would disagree.
	const TArray<FName> Keys = {TEXT("Apple"), TEXT("Cherry"), TEXT("Foo_10"), TEXT("Foo_9"), TEXT("banana")};
	UDrTableTestAsset* Asset = NewObject<UDrTableTestAsset>(GetTransientPackage(), TEXT("DrTableNameOrdering"));
	Asset->Code_Keys = Keys;
	for (int32 Index = 0; Index < Keys.Num(); ++Index)
	{
		FDrTableTestRow& Row = Asset->Rows.AddDefaulted_GetRef();
		Row.Id = Index;
		Row.Code = Keys[Index];
		Asset->Code_Offsets.Add(Index);
		Asset->Code_Indices.Add(Index);
	}
	Asset->Code_Offsets.Add(Keys.Num());

	UDrTableRegistry* Registry = DrTableTests::NewIsolatedRegistry();
	Registry->Register<FDrTableTestRow, UDrTableTestAsset>(Asset->GetFName(), &UDrTableTestAsset::Rows, &UDrTableTestAsset::Code_Keys)
		.WithUniqueSubKey(TEXT("Code"), &UDrTableTestAsset::Code_Keys, &UDrTableTestAsset::Code_Offsets, &UDrTableTestAsset::Code_Indices);
	Registry->ExtraAssets.Add(TSoftObjectPtr<UPrimaryDataAsset>(Asset));
	Registry->LoadAllTables();

	for (int32 Index = 0; Index < Keys.Num(); ++Index)
	{
		const FString Name = Keys[Index].ToString();
		const FDrTableTestRow* Row = Registry->FindRowByKey<FDrTableTestRow>(Keys[Index]);
		TestNotNull(*FString::Printf(TEXT("name primary key %s"), *Name), Row);
		if (Row)
		{
			TestEqual(*FString::Printf(TEXT("name primary key row %s"), *Name), Row->Id, Index);
		}
		TestEqual(*FString::Printf(TEXT("name sub key %s"), *Name), Registry->FindRowsBySubKey<FDrTableTestRow>(TEXT("Code"), Keys[Index]).Num(), 1);
	}
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrTableSubKeyNotDeclaredTest, "DrTable.Registry.SubKey.NotDeclared", DrTableTests::Flags)
bool FDrTableSubKeyNotDeclaredTest::RunTest(const FString&)
{
	UDrTableNoSubKeyTestAsset* Asset = NewObject<UDrTableNoSubKeyTestAsset>(GetTransientPackage(), TEXT("DrTableNoSubKey"));
	FDrTableTestRow Row;
	Row.Id = 10;
	Asset->Rows.Add(Row);
	Asset->PrimaryKeys.Add(10);
	UDrTableRegistry* Registry = DrTableTests::NewIsolatedRegistry();
	Registry->Register<FDrTableTestRow, UDrTableNoSubKeyTestAsset>(Asset->GetFName(), &UDrTableNoSubKeyTestAsset::Rows, &UDrTableNoSubKeyTestAsset::PrimaryKeys);
	Registry->ExtraAssets.Add(TSoftObjectPtr<UPrimaryDataAsset>(Asset));
	Registry->LoadAllTables();
	TestEqual(TEXT("undeclared sub key is empty"), Registry->FindRowsBySubKey<FDrTableTestRow>(TEXT("Missing"), 123).Num(), 0);
	TestNotNull(TEXT("primary key still works"), Registry->FindRowByKey<FDrTableTestRow>(10));
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrTableReloadTest, "DrTable.Registry.ReloadUsesBakedIndices", DrTableTests::Flags)
bool FDrTableReloadTest::RunTest(const FString&)
{
	DrTableTests::FFixture Fixture;
	Fixture.Asset->Rows.RemoveAt(0);
	Fixture.Asset->PrimaryKeys.RemoveAt(0);
	Fixture.Asset->Group_Offsets = {0, 1, 1};
	Fixture.Asset->Group_Indices = {0};
	Fixture.Asset->Code_Keys = {TEXT("SecondCode")};
	Fixture.Asset->Code_Offsets = {0, 1};
	Fixture.Asset->Code_Indices = {0};
	Fixture.Registry->ReloadAllTables();
	TestNull(TEXT("removed key after reload"), Fixture.Registry->FindRowByKey<FDrTableTestRow>(10));
	TestEqual(TEXT("baked CSR after reload"), Fixture.Registry->FindRowsBySubKey<FDrTableTestRow>(TEXT("Group"), EDrTableTestGroup::ZShared).Num(), 1);
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrTableInvalidIndexTest, "DrTable.Registry.InvalidIndexIsDisabled", DrTableTests::Flags)
bool FDrTableInvalidIndexTest::RunTest(const FString&)
{
	DrTableTests::FFixture Fixture;
	Fixture.Asset->Group_Offsets.Reset();
	Fixture.Asset->Group_Indices.Reset();
	AddExpectedMessage(TEXT("Invalid CSR index lengths"), ELogVerbosity::Warning, EAutomationExpectedErrorFlags::Contains, 1);
	Fixture.Registry->ReloadAllTables();
	TestEqual(TEXT("broken index is not rebuilt at runtime"),
		Fixture.Registry->FindRowsBySubKey<FDrTableTestRow>(TEXT("Group"), EDrTableTestGroup::ZShared).Num(), 0);
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrTableAmbiguousTest, "DrTable.Registry.AmbiguousRowType", DrTableTests::Flags)
bool FDrTableAmbiguousTest::RunTest(const FString&)
{
	DrTableTests::FFixture Fixture;
	UDrTableTestAsset* Second = NewObject<UDrTableTestAsset>(GetTransientPackage(), TEXT("DrTableSecond"));
	DrTableTests::FillAsset(*Second);
	DrTableTests::RegisterAsset(*Fixture.Registry, Second->GetFName());
	Fixture.Registry->ExtraAssets.Add(TSoftObjectPtr<UPrimaryDataAsset>(Second));
	Fixture.Registry->ReloadAllTables();
	AddExpectedMessage(TEXT("registered for more than one table"), ELogVerbosity::Warning, EAutomationExpectedErrorFlags::Contains, 1);
	TestEqual(TEXT("type-only lookup refuses to guess"), Fixture.Registry->GetRows<FDrTableTestRow>().Num(), 0);
	TestNotNull(TEXT("lookup by table id still works"), Fixture.Registry->FindRowByKey<FDrTableTestRow>(Second->GetFName(), 20));
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrTableSchemaHashTest, "DrTable.Registry.SchemaHash", DrTableTests::Flags)
bool FDrTableSchemaHashTest::RunTest(const FString&)
{
	DrTableTests::FFixture Fixture;
	DrTableTests::RegisterAsset(*Fixture.Registry, Fixture.Id()).WithSchemaHash(TEXT("sha256:schema"));
	Fixture.Asset->SchemaHash = TEXT("sha256:schema");
	Fixture.Registry->ReloadAllTables();
	TestNotNull(TEXT("matching schema loads"), Fixture.Registry->FindRowByKey<FDrTableTestRow>(20));

	AddExpectedError(TEXT("Schema mismatch, re-bake required"), EAutomationExpectedErrorFlags::Contains, 2);
	Fixture.Asset->SchemaHash = TEXT("sha256:old");
	Fixture.Registry->ReloadAllTables();
	TestNull(TEXT("different schema is not loaded"), Fixture.Registry->FindRowByKey<FDrTableTestRow>(20));
	Fixture.Asset->SchemaHash.Reset();
	Fixture.Registry->ReloadAllTables();
	TestNull(TEXT("empty schema is not loaded"), Fixture.Registry->FindRowByKey<FDrTableTestRow>(20));
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrTableContentHashTest, "DrTable.Registry.ContentHash", DrTableTests::Flags)
bool FDrTableContentHashTest::RunTest(const FString&)
{
	DrTableTests::FFixture Fixture;
	DrTableTests::RegisterAsset(*Fixture.Registry, Fixture.Id()).WithContentHash(TEXT("sha256:new-data"));
	Fixture.Asset->ContentHash = TEXT("sha256:new-data");
	Fixture.Registry->ReloadAllTables();
	TestNotNull(TEXT("matching content loads"), Fixture.Registry->FindRowByKey<FDrTableTestRow>(20));

	AddExpectedMessage(TEXT("Data changed since the asset was baked"), ELogVerbosity::Warning, EAutomationExpectedErrorFlags::Contains, 1);
	Fixture.Asset->ContentHash = TEXT("sha256:old-data");
	Fixture.Registry->ReloadAllTables();
	TestNotNull(TEXT("stale content still loads (warning only)"), Fixture.Registry->FindRowByKey<FDrTableTestRow>(20));
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrTableRuntimeContractTest, "DrTable.Runtime.Contract", DrTableTests::Flags)
bool FDrTableRuntimeContractTest::RunTest(const FString&)
{
	DrTableTests::FFixture Fixture;
	{
		DrTableTests::FScopedOverride Scope(Fixture.Registry);
		const FDrTableTestRow* Row = DrTableRuntime::FindByKey<FDrTableTestRow>(20);
		TestNotNull(TEXT("FindByKey"), Row);
		TestEqual(TEXT("FindAllBySubKey"), DrTableRuntime::FindAllBySubKey<FDrTableTestRow>(TEXT("Group"), EDrTableTestGroup::ZShared).Num(), 2);
		TestEqual(TEXT("GetAll"), DrTableRuntime::GetAll<FDrTableTestRow>().Num(), 2);
	}
	{
		DrTableTests::FScopedOverride Scope(nullptr);
		TestNull(TEXT("no registry: FindByKey"), DrTableRuntime::FindByKey<FDrTableTestRow>(20));
		TestEqual(TEXT("no registry: FindAllBySubKey"), DrTableRuntime::FindAllBySubKey<FDrTableTestRow>(TEXT("Group"), EDrTableTestGroup::ZShared).Num(), 0);
		TestEqual(TEXT("no registry: GetAll"), DrTableRuntime::GetAll<FDrTableTestRow>().Num(), 0);
	}
	return true;
}

#endif // WITH_DEV_AUTOMATION_TESTS
