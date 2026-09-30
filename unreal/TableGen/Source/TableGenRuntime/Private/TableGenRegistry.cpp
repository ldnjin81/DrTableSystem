// Copyright ldnjin81. All Rights Reserved.

#include "TableGenRegistry.h"

#include "Engine/Engine.h"
#include "TableGenAssetBase.h"
#include "TableGenSettings.h"

DEFINE_LOG_CATEGORY(LogTableGen);

namespace TableGen::Private
{
	TArray<FTableGenRegisterFunction>& AutoRegisterFunctions()
	{
		static TArray<FTableGenRegisterFunction> Functions;
		return Functions;
	}

	bool bOverrideActive = false;
	TWeakObjectPtr<UTableGenRegistry> OverrideRegistry;
} // namespace TableGen::Private

FTableGenAutoRegister::FTableGenAutoRegister(FTableGenRegisterFunction Function)
{
	TableGen::Private::AutoRegisterFunctions().AddUnique(Function);
}

const TArray<FTableGenRegisterFunction>& FTableGenAutoRegister::GetFunctions()
{
	return TableGen::Private::AutoRegisterFunctions();
}

UTableGenRegistry* UTableGenRegistry::Get()
{
	if (TableGen::Private::bOverrideActive)
	{
		return TableGen::Private::OverrideRegistry.Get();
	}
	UTableGenSubsystem* Subsystem = GEngine ? GEngine->GetEngineSubsystem<UTableGenSubsystem>() : nullptr;
	UTableGenRegistry* Registry = Subsystem ? Subsystem->GetRegistry() : nullptr;
	if (Registry && !Registry->bHasLoaded && GetDefault<UTableGenSettings>()->bLoadOnFirstUse)
	{
		// Modules loaded after the subsystem started may have added registrations since.
		Registry->RegisterAutoTables();
		Registry->LoadAllTables();
	}
	return Registry;
}

void UTableGenRegistry::SetOverride(UTableGenRegistry* Registry)
{
	TableGen::Private::bOverrideActive = true;
	TableGen::Private::OverrideRegistry = Registry;
}

void UTableGenRegistry::ClearOverride()
{
	TableGen::Private::bOverrideActive = false;
	TableGen::Private::OverrideRegistry.Reset();
}

void UTableGenRegistry::PostInitProperties()
{
	Super::PostInitProperties();
	if (!HasAnyFlags(RF_ClassDefaultObject))
	{
		const UTableGenSettings* Settings = GetDefault<UTableGenSettings>();
		AssetRoot = Settings->AssetRoot;
		ExtraAssets = Settings->ExtraAssets;
	}
}

void UTableGenRegistry::RegisterAutoTables()
{
	for (const FTableGenRegisterFunction Function : FTableGenAutoRegister::GetFunctions())
	{
		Function(*this);
	}
}

void UTableGenRegistry::LoadAllTables()
{
	bHasLoaded = true;
	for (TPair<FName, TUniquePtr<FTableGenRowTableBase>>& Pair : Tables)
	{
		Pair.Value->Reset();
	}
	LoadedAssets.Reset();

	// Automatic path per registered table, unless an extra asset with the same name overrides it.
	TArray<TSoftObjectPtr<UPrimaryDataAsset>> LoadList;
	if (!AssetRoot.IsEmpty())
	{
		for (const TPair<FName, TUniquePtr<FTableGenRowTableBase>>& Pair : Tables)
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

	const bool bWarnOnContent = GetDefault<UTableGenSettings>()->bWarnOnContentMismatch;
	for (const TSoftObjectPtr<UPrimaryDataAsset>& Ref : LoadList)
	{
		if (Ref.IsNull())
		{
			continue;
		}
		UPrimaryDataAsset* Asset = Ref.LoadSynchronous();
		if (!Asset)
		{
			UE_LOG(LogTableGen, Warning, TEXT("[TableGen] Table asset not found: %s (bake it with -run=TableGenBake)"), *Ref.ToSoftObjectPath().ToString());
			continue;
		}
		FTableGenRowTableBase* Destination = ResolveTable(*Asset);
		if (!Destination)
		{
			UE_LOG(LogTableGen, Warning, TEXT("[TableGen] Asset is not registered or matches more than one table: %s"), *Asset->GetName());
			continue;
		}

		const UTableGenAssetBase* Baked = Cast<UTableGenAssetBase>(Asset);
		const FString& ExpectedSchema = Destination->GetExpectedSchemaHash();
		if (!ExpectedSchema.IsEmpty() && (!Baked || Baked->SchemaHash != ExpectedSchema))
		{
			UE_LOG(LogTableGen, Error, TEXT("[TableGen] Schema mismatch, re-bake required: %s (asset: %s, code: %s)"),
				*Asset->GetName(), Baked && !Baked->SchemaHash.IsEmpty() ? *Baked->SchemaHash : TEXT("<none>"), *ExpectedSchema);
			continue;
		}
		const FString& ExpectedContent = Destination->GetExpectedContentHash();
		if (bWarnOnContent && !ExpectedContent.IsEmpty() && Baked && Baked->ContentHash != ExpectedContent)
		{
			UE_LOG(LogTableGen, Warning, TEXT("[TableGen] Data changed since the asset was baked, re-bake to apply: %s (asset: %s, code: %s)"),
				*Asset->GetName(), Baked->ContentHash.IsEmpty() ? TEXT("<none>") : *Baked->ContentHash, *ExpectedContent);
		}

		if (!Destination->Load(*Asset))
		{
			UE_LOG(LogTableGen, Warning, TEXT("[TableGen] Asset class or array layout does not match the registration: %s"), *Asset->GetName());
			continue;
		}
		LoadedAssets.Add(Asset);
	}
}

void UTableGenRegistry::DumpStatus(FOutputDevice& Out) const
{
	TArray<FName> Ids;
	Tables.GetKeys(Ids);
	Ids.Sort(FNameLexicalLess());
	Out.Logf(TEXT("TableGen: %d table(s) registered, assets root '%s'"), Ids.Num(), *AssetRoot);
	for (const FName Id : Ids)
	{
		const FTableGenRowTableBase& Table = *Tables[Id];
		Out.Logf(TEXT("  %-32s %-32s %s"), *Id.ToString(), *GetNameSafe(Table.GetRowStruct()),
			Table.IsLoaded() ? *FString::Printf(TEXT("%d row(s)"), Table.Num()) : TEXT("not loaded"));
	}
}

const FTableGenRowTableBase* UTableGenRegistry::FindTableByStruct(const UScriptStruct* RowStruct) const
{
	FTableGenRowTableBase* const* Found = TablesByStruct.Find(RowStruct);
	if (!Found)
	{
		return nullptr;
	}
	if (*Found == nullptr)
	{
		UE_LOG(LogTableGen, Warning, TEXT("[TableGen] Row type is registered for more than one table, look it up by table id: %s"), *GetNameSafe(RowStruct));
	}
	return *Found;
}

void UTableGenRegistry::RebuildTypeIndex()
{
	TablesByStruct.Reset();
	for (TPair<FName, TUniquePtr<FTableGenRowTableBase>>& Pair : Tables)
	{
		const UScriptStruct* RowStruct = Pair.Value->GetRowStruct();
		if (FTableGenRowTableBase** Existing = TablesByStruct.Find(RowStruct))
		{
			*Existing = nullptr; // ambiguous
		}
		else
		{
			TablesByStruct.Add(RowStruct, Pair.Value.Get());
		}
	}
}

FTableGenRowTableBase* UTableGenRegistry::ResolveTable(const UPrimaryDataAsset& Source) const
{
	if (const TUniquePtr<FTableGenRowTableBase>* ByName = Tables.Find(Source.GetFName()))
	{
		return ByName->Get();
	}
	FTableGenRowTableBase* Match = nullptr;
	for (const TPair<FName, TUniquePtr<FTableGenRowTableBase>>& Pair : Tables)
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

void UTableGenSubsystem::Initialize(FSubsystemCollectionBase& Collection)
{
	Super::Initialize(Collection);
	Registry = NewObject<UTableGenRegistry>(this);
	Registry->RegisterAutoTables();
}

void UTableGenSubsystem::Deinitialize()
{
	Registry = nullptr;
	Super::Deinitialize();
}
