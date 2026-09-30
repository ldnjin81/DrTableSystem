// Copyright ldnjin81. All Rights Reserved.

#include "HAL/IConsoleManager.h"
#include "Modules/ModuleManager.h"
#include "TableGenRegistry.h"
#include "TableGenSettings.h"

UTableGenSettings::UTableGenSettings()
	: AssetRoot(TEXT("/Game/Data"))
{
}

namespace TableGen::Private
{
	static FAutoConsoleCommandWithOutputDevice StatusCommand(
		TEXT("TableGen.Status"),
		TEXT("Lists the registered tables and how many rows each has loaded."),
		FConsoleCommandWithOutputDeviceDelegate::CreateLambda([](FOutputDevice& Out)
		{
			if (const UTableGenRegistry* Registry = UTableGenRegistry::Get())
			{
				Registry->DumpStatus(Out);
			}
			else
			{
				Out.Log(TEXT("TableGen: no active registry."));
			}
		}));

	static FAutoConsoleCommand ReloadCommand(
		TEXT("TableGen.Reload"),
		TEXT("Reloads every registered table from its baked asset. Row pointers obtained before the reload become invalid."),
		FConsoleCommandDelegate::CreateLambda([]()
		{
			if (UTableGenRegistry* Registry = UTableGenRegistry::Get())
			{
				Registry->RegisterAutoTables();
				Registry->ReloadAllTables();
			}
		}));
} // namespace TableGen::Private

IMPLEMENT_MODULE(FDefaultModuleImpl, TableGenRuntime)
