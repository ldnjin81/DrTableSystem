// Copyright ldnjin81. All Rights Reserved.

#include "Modules/ModuleManager.h"
#include "DrTableCook.h"

class FDrTableEditorModule : public IModuleInterface
{
public:
	virtual void StartupModule() override
	{
		CookHandle = UE::Cook::FDelegates::ModifyCook.AddStatic(&DrTableCook::ModifyCook);
	}
	virtual void ShutdownModule() override
	{
		UE::Cook::FDelegates::ModifyCook.Remove(CookHandle);
	}
private:
	FDelegateHandle CookHandle;
};

IMPLEMENT_MODULE(FDrTableEditorModule, DrTableEditor)
