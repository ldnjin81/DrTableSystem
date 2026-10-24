// Copyright ldnjin81. All Rights Reserved.

#if WITH_DEV_AUTOMATION_TESTS

#include "Misc/AutomationTest.h"
#include "DrTableCook.h"
#include "DrTableSettings.h"
#include "AssetRegistry/AssetRegistryModule.h"
#include "DrStringTableAsset.h"
#include "Tests/DrTableTestTypes.h"
#include "UObject/Package.h"

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrTableCookPackagesTest, "DrTable.Cook.Packages",
	EAutomationTestFlags_ApplicationContextMask | EAutomationTestFlags::ProductFilter)
bool FDrTableCookPackagesTest::RunTest(const FString&)
{
	TestTrue(TEXT("ModifyCook callback registered"), UE::Cook::FDelegates::ModifyCook.IsBound());
	UDrTableSettings* Settings = NewObject<UDrTableSettings>();
	Settings->AssetRoot = TEXT("/DrTableTest/Root");
	UDrTableTestAsset* Table = NewObject<UDrTableTestAsset>(CreatePackage(TEXT("/DrTableTest/Root/DA_Table")), TEXT("DA_Table"), RF_Public | RF_Standalone);
	UDrStringManifest* Manifest = NewObject<UDrStringManifest>(CreatePackage(TEXT("/DrTableTest/Root/Strings/DA_DrStrings")), TEXT("DA_DrStrings"), RF_Public | RF_Standalone);
	FAssetRegistryModule::AssetCreated(Table);
	FAssetRegistryModule::AssetCreated(Manifest);
	const FSoftObjectPath Extra(TEXT("/DrTableTest/Outside/DA_Extra.DA_Extra"));
	Settings->ExtraAssets = {TSoftObjectPtr<UPrimaryDataAsset>(Extra), TSoftObjectPtr<UPrimaryDataAsset>(Extra)};
	TArray<FName> Packages;
	DrTableCook::GatherPackages(*Settings, Packages);
	TestEqual(TEXT("Tables and strings under the root plus extra assets"), Packages.Num(), 3);
	TestTrue(TEXT("Table under the root"), Packages.Contains(Table->GetOutermost()->GetFName()));
	TestTrue(TEXT("String manifest"), Packages.Contains(Manifest->GetOutermost()->GetFName()));
	TestTrue(TEXT("ExtraAssets package name"), Packages.Contains(Extra.GetLongPackageFName()));
	FAssetRegistryModule::AssetDeleted(Table);
	FAssetRegistryModule::AssetDeleted(Manifest);
	Table->ClearFlags(RF_Public | RF_Standalone);
	Manifest->ClearFlags(RF_Public | RF_Standalone);
	Table->MarkAsGarbage();
	Manifest->MarkAsGarbage();
	return true;
}

IMPLEMENT_SIMPLE_AUTOMATION_TEST(FDrTablePathValidationTest, "DrTable.Cook.PathValidation",
	EAutomationTestFlags_ApplicationContextMask | EAutomationTestFlags::ProductFilter)
bool FDrTablePathValidationTest::RunTest(const FString&)
{
	TestTrue(TEXT("Empty path is allowed"), DrTableCook::IsValidAssetPath(FSoftObjectPath()));
	TestFalse(TEXT("Missing asset is rejected"), DrTableCook::IsValidAssetPath(FSoftObjectPath(TEXT("/DrTableTest/Missing.Missing"))));
	TestFalse(TEXT("Missing generated class is rejected"), DrTableCook::IsValidAssetPath(FSoftObjectPath(TEXT("/DrTableTest/Missing.Missing_C"))));
	TestTrue(TEXT("Engine asset is allowed"), DrTableCook::IsValidAssetPath(FSoftObjectPath(TEXT("/Engine/EngineResources/DefaultTexture.DefaultTexture"))));
	return true;
}
#endif
