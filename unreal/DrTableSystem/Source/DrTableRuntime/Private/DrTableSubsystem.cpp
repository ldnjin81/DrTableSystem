// Copyright ldnjin81. All Rights Reserved.

#include "DrTableSubsystem.h"

#include "Engine/Engine.h"
#include "DrTableAssetBase.h"
#include "DrTableSettings.h"

DEFINE_LOG_CATEGORY(LogDrTable);

namespace DrTable::Private
{
	TArray<FDrTableRegisterFunction>& AutoRegisterFunctions()
	{
		static TArray<FDrTableRegisterFunction> Functions;
		return Functions;
	}

	bool bOverrideActive = false;
	TWeakObjectPtr<UDrTableSubsystem> OverrideTables;
} // namespace DrTable::Private

FDrTableAutoRegister::FDrTableAutoRegister(FDrTableRegisterFunction Function)
{
	DrTable::Private::AutoRegisterFunctions().AddUnique(Function);
}

const TArray<FDrTableRegisterFunction>& FDrTableAutoRegister::GetFunctions()
{
	return DrTable::Private::AutoRegisterFunctions();
}

UDrTableSubsystem* UDrTableSubsystem::Get()
{
	if (DrTable::Private::bOverrideActive)
	{
		return DrTable::Private::OverrideTables.Get();
	}
	UDrTableSubsystem* Tables = GEngine ? GEngine->GetEngineSubsystem<UDrTableSubsystem>() : nullptr;
	if (Tables && !Tables->bHasLoaded && GetDefault<UDrTableSettings>()->bLoadOnFirstUse)
	{
		// Modules loaded after the subsystem started may have added registrations since.
		Tables->RegisterAutoTables();
		Tables->LoadAllTables();
	}
	return Tables;
}

void UDrTableSubsystem::SetOverride(UDrTableSubsystem* Tables)
{
	DrTable::Private::bOverrideActive = true;
	DrTable::Private::OverrideTables = Tables;
}

void UDrTableSubsystem::ClearOverride()
{
	DrTable::Private::bOverrideActive = false;
	DrTable::Private::OverrideTables.Reset();
}

void UDrTableSubsystem::PostInitProperties()
{
	Super::PostInitProperties();
	if (!HasAnyFlags(RF_ClassDefaultObject))
	{
		const UDrTableSettings* Settings = GetDefault<UDrTableSettings>();
		AssetRoot = Settings->AssetRoot;
		ExtraAssets = Settings->ExtraAssets;
	}
}

void UDrTableSubsystem::RegisterAutoTables()
{
	for (const FDrTableRegisterFunction Function : FDrTableAutoRegister::GetFunctions())
	{
		Function(*this);
	}
}

void UDrTableSubsystem::LoadAllTables()
{
	bHasLoaded = true;
	for (TPair<FName, TUniquePtr<FDrTableRowTableBase>>& Pair : Tables)
	{
		Pair.Value->Reset();
	}
	LoadedAssets.Reset();

	// Automatic path per registered table, unless an extra asset with the same name overrides it.
	TArray<TSoftObjectPtr<UPrimaryDataAsset>> LoadList;
	if (!AssetRoot.IsEmpty())
	{
		for (const TPair<FName, TUniquePtr<FDrTableRowTableBase>>& Pair : Tables)
		{
			const FString Id = Pair.Key.ToString();
			const bool bOverridden = ExtraAssets.ContainsByPredicate([&Id](const TSoftObjectPtr<UPrimaryDataAsset>& Ref)
			{
				return Ref.ToSoftObjectPath().GetAssetName() == Id;
			});
			if (!bOverridden)
			{
				LoadList.Add(TSoftObjectPtr<UPrimaryDataAsset>(FSoftObjectPath(FString::Printf(TEXT("%s/%s.%s"), *AssetRoot, *Id, *Id))));
			}
		}
	}
	LoadList.Append(ExtraAssets);

	const bool bWarnOnContent = GetDefault<UDrTableSettings>()->bWarnOnContentMismatch;
	for (const TSoftObjectPtr<UPrimaryDataAsset>& Ref : LoadList)
	{
		if (Ref.IsNull())
		{
			continue;
		}
		UPrimaryDataAsset* Asset = Ref.LoadSynchronous();
		if (!Asset)
		{
			UE_LOG(LogDrTable, Warning, TEXT("[DrTable] Table asset not found: %s (bake it with -run=DrTableBake)"), *Ref.ToSoftObjectPath().ToString());
			continue;
		}
		FDrTableRowTableBase* Destination = ResolveTable(*Asset);
		if (!Destination)
		{
			UE_LOG(LogDrTable, Warning, TEXT("[DrTable] Asset is not registered or matches more than one table: %s"), *Asset->GetName());
			continue;
		}

		const UDrTableAssetBase* Baked = Cast<UDrTableAssetBase>(Asset);
		const FString& ExpectedSchema = Destination->GetExpectedSchemaHash();
		if (!ExpectedSchema.IsEmpty() && (!Baked || Baked->SchemaHash != ExpectedSchema))
		{
			UE_LOG(LogDrTable, Error, TEXT("[DrTable] Schema mismatch, re-bake required: %s (asset: %s, code: %s)"),
				*Asset->GetName(), Baked && !Baked->SchemaHash.IsEmpty() ? *Baked->SchemaHash : TEXT("<none>"), *ExpectedSchema);
			continue;
		}
		const FString& ExpectedContent = Destination->GetExpectedContentHash();
		if (bWarnOnContent && !ExpectedContent.IsEmpty() && Baked && Baked->ContentHash != ExpectedContent)
		{
			UE_LOG(LogDrTable, Warning, TEXT("[DrTable] Data changed since the asset was baked, re-bake to apply: %s (asset: %s, code: %s)"),
				*Asset->GetName(), Baked->ContentHash.IsEmpty() ? TEXT("<none>") : *Baked->ContentHash, *ExpectedContent);
		}

		if (!Destination->Load(*Asset))
		{
			UE_LOG(LogDrTable, Warning, TEXT("[DrTable] Asset class or array layout does not match the registration: %s"), *Asset->GetName());
			continue;
		}
		LoadedAssets.Add(Asset);
	}
}

void UDrTableSubsystem::DumpStatus(FOutputDevice& Out) const
{
	TArray<FName> Ids;
	Tables.GetKeys(Ids);
	Ids.Sort(FNameLexicalLess());
	Out.Logf(TEXT("DrTable: %d table(s) registered, assets root '%s'"), Ids.Num(), *AssetRoot);
	for (const FName Id : Ids)
	{
		const FDrTableRowTableBase& Table = *Tables[Id];
		Out.Logf(TEXT("  %-32s %-32s %s"), *Id.ToString(), *GetNameSafe(Table.GetRowStruct()),
			Table.IsLoaded() ? *FString::Printf(TEXT("%d row(s)"), Table.Num()) : TEXT("not loaded"));
	}
}

bool UDrTableSubsystem::AreAllTablesLoaded() const
{
	if (Tables.IsEmpty()) return false;
	for (const auto& Pair : Tables)
	{
		if (!Pair.Value->IsLoaded()) return false;
	}
	return true;
}

const FDrTableRowTableBase* UDrTableSubsystem::FindTableByStruct(const UScriptStruct* RowStruct) const
{
	FDrTableRowTableBase* const* Found = TablesByStruct.Find(RowStruct);
	if (!Found)
	{
		return nullptr;
	}
	if (*Found == nullptr)
	{
		UE_LOG(LogDrTable, Warning, TEXT("[DrTable] Row type is registered for more than one table, look it up by table id: %s"), *GetNameSafe(RowStruct));
	}
	return *Found;
}

void UDrTableSubsystem::RebuildTypeIndex()
{
	TablesByStruct.Reset();
	for (TPair<FName, TUniquePtr<FDrTableRowTableBase>>& Pair : Tables)
	{
		const UScriptStruct* RowStruct = Pair.Value->GetRowStruct();
		if (FDrTableRowTableBase** Existing = TablesByStruct.Find(RowStruct))
		{
			*Existing = nullptr; // ambiguous
		}
		else
		{
			TablesByStruct.Add(RowStruct, Pair.Value.Get());
		}
	}
}

FDrTableRowTableBase* UDrTableSubsystem::ResolveTable(const UPrimaryDataAsset& Source) const
{
	if (const TUniquePtr<FDrTableRowTableBase>* ByName = Tables.Find(Source.GetFName()))
	{
		return ByName->Get();
	}
	FDrTableRowTableBase* Match = nullptr;
	for (const TPair<FName, TUniquePtr<FDrTableRowTableBase>>& Pair : Tables)
	{
		if (!Source.IsA(Pair.Value->GetAssetClass()))
		{
			continue;
		}
		if (Match)
		{
			return nullptr;
		}
		Match = Pair.Value.Get();
	}
	return Match;
}

void UDrTableSubsystem::Initialize(FSubsystemCollectionBase& Collection)
{
	Super::Initialize(Collection);
	RegisterAutoTables();
}
