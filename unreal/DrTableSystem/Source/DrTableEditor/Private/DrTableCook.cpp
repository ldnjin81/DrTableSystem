// Copyright ldnjin81. All Rights Reserved.

#include "DrTableCook.h"

#include "AssetRegistry/AssetRegistryModule.h"
#include "DrTableAssetBase.h"
#include "DrTableSettings.h"
#include "DrStringTableAsset.h"
#include "Engine/Blueprint.h"
#include "Modules/ModuleManager.h"
#include "UObject/UnrealType.h"

DEFINE_LOG_CATEGORY_STATIC(LogDrTableCook, Log, All);

void DrTableCook::GatherPackages(const UDrTableSettings& Settings, TArray<FName>& OutPackages)
{
	IAssetRegistry& Registry = FModuleManager::LoadModuleChecked<FAssetRegistryModule>(TEXT("AssetRegistry")).Get();
	Registry.SearchAllAssets(true);
	if (!Settings.AssetRoot.IsEmpty())
	{
		FARFilter Filter;
		Filter.PackagePaths.Add(FName(Settings.AssetRoot));
		Filter.bRecursivePaths = true;
		Filter.ClassPaths = {UDrTableAssetBase::StaticClass()->GetClassPathName(),
			UDrStringManifest::StaticClass()->GetClassPathName(), UDrStringTableAsset::StaticClass()->GetClassPathName()};
		Filter.bRecursiveClasses = true;
		TArray<FAssetData> Assets;
		Registry.GetAssets(Filter, Assets);
		for (const FAssetData& Asset : Assets) OutPackages.AddUnique(Asset.PackageName);
	}
	for (const TSoftObjectPtr<UPrimaryDataAsset>& Extra : Settings.ExtraAssets)
	{
		if (!Extra.IsNull()) OutPackages.AddUnique(Extra.ToSoftObjectPath().GetLongPackageFName());
	}
}

void DrTableCook::ModifyCook(UE::Cook::ICookInfo&, TArray<UE::Cook::FPackageCookRule>& Rules)
{
	TArray<FName> Packages;
	GatherPackages(*GetDefault<UDrTableSettings>(), Packages);
	for (FName Package : Packages)
	{
		UE::Cook::FPackageCookRule Rule;
		Rule.PackageName = Package;
		Rule.InstigatorName = TEXT("DrTable");
		Rule.CookRule = UE::Cook::EPackageCookRule::AddToCook;
		Rules.Add(Rule);
	}
}

bool DrTableCook::IsValidAssetPath(const FSoftObjectPath& Path)
{
	if (Path.IsNull()) return true;
	IAssetRegistry& Registry = FModuleManager::LoadModuleChecked<FAssetRegistryModule>(TEXT("AssetRegistry")).Get();
	if (Registry.GetAssetByObjectPath(Path).IsValid()) return true;
	// A generated class is not a registry asset of its own, so check its source Blueprint.
	const FString ObjectPath = Path.ToString();
	if (!Path.GetSubPathString().IsEmpty() || !ObjectPath.EndsWith(TEXT("_C"))) return false;
	const FAssetData Blueprint = Registry.GetAssetByObjectPath(FSoftObjectPath(ObjectPath.LeftChop(2)));
	return Blueprint.IsValid() && Blueprint.IsInstanceOf(UBlueprint::StaticClass());
}

namespace
{
	bool ValidateValue(const FProperty& Property, const void* Value, const FString& Location)
	{
		if (const FArrayProperty* Array = CastField<FArrayProperty>(&Property))
		{
			FScriptArrayHelper Helper(Array, Value);
			bool bValid = true;
			for (int32 Index = 0; Index < Helper.Num(); ++Index)
				bValid &= ValidateValue(*Array->Inner, Helper.GetRawPtr(Index), FString::Printf(TEXT("%s[%d]"), *Location, Index));
			return bValid;
		}
		if (const FStructProperty* Struct = CastField<FStructProperty>(&Property))
		{
			if (Struct->Struct == TBaseStructure<FSoftObjectPath>::Get())
			{
				const FSoftObjectPath& Path = *static_cast<const FSoftObjectPath*>(Value);
				if (DrTableCook::IsValidAssetPath(Path)) return true;
				UE_LOG(LogDrTableCook, Error, TEXT("[%s] Asset path does not exist: %s"), *Location, *Path.ToString());
				return false;
			}
			bool bValid = true;
			for (TFieldIterator<FProperty> It(Struct->Struct); It; ++It)
				bValid &= ValidateValue(**It, It->ContainerPtrToValuePtr<void>(Value), Location + TEXT(".") + It->GetName());
			return bValid;
		}
		return true;
	}
}

bool DrTableCook::ValidatePaths(const UObject& Asset, const FString& Table)
{
	IAssetRegistry& Registry = FModuleManager::LoadModuleChecked<FAssetRegistryModule>(TEXT("AssetRegistry")).Get();
	Registry.SearchAllAssets(true);
	bool bValid = true;
	for (TFieldIterator<FArrayProperty> It(Asset.GetClass()); It; ++It)
		bValid &= ValidateValue(**It, It->ContainerPtrToValuePtr<void>(&Asset), Table + TEXT(".") + It->GetName());
	return bValid;
}
